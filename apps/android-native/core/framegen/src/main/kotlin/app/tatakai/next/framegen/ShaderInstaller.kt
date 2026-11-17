/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.content.Context
import android.net.Uri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.security.MessageDigest
import java.time.Instant

/** R13. Largest `Lossless.dll` accepted for import (64 MB). */
const val LSFG_DLL_MAX_BYTES: Long = 64L * 1024 * 1024

/**
 * R13. Outcome of [ShaderInstaller.install]. The installer never touches strings; the UI maps each
 * value to its `fg_install_*` message.
 */
enum class InstallResult { IMPORTED, TOO_LARGE, UNREADABLE, MISSING_SHADERS, WRITE_FAILED, UNAVAILABLE }

/**
 * R13. Contents of `filesDir/lsfg/spirv/manifest.json`, written for every successful import.
 *
 * @property sha256 lowercase hex SHA-256 of the imported DLL (the DLL itself is not kept).
 * @property fp32Count FP32 shaders written by [FgNative.extractShaders].
 * @property fp16Present whether `spirv/fp16` holds at least one shader.
 * @property importedAt ISO-8601 instant of the import (`Instant.toString()`).
 */
@Serializable
data class ShaderManifest(
  val sha256: String,
  val fp32Count: Int,
  val fp16Present: Boolean,
  val importedAt: String,
)

/**
 * R13 frame generation: imports the user's `Lossless.dll` and keeps only its SPIR-V shaders.
 *
 * Pattern of `HrtfAssetInstaller` (Sofalizer.kt): bounded copy to a temporary file, result enum,
 * temporaries removed in `finally`, only exact paths below the owned directories are deleted.
 *
 * Facts 10-12:
 * - The DLL is never kept: it is copied to `cacheDir/lsfg-import/Lossless.dll.tmp` (at most
 *   [LSFG_DLL_MAX_BYTES]) and that file is deleted in `finally` on every path, success or failure.
 * - The `.spv` files only live in app-private storage, `filesDir/lsfg/spirv/{fp32,fp16}/`, next to
 *   `manifest.json` ([ShaderManifest]). Nothing is written outside `cacheDir` / `filesDir`.
 * - The native extraction writes file by file and may leave a partial set on failure, so it always
 *   targets the staging directory `filesDir/lsfg/spirv.staging`. Only when the code is OK and
 *   [FgNative.probeShaders] accepts the staging set is the manifest added there, the previous
 *   `filesDir/lsfg/spirv` removed and the staging directory renamed onto it (a directory rename,
 *   so readers see either the old complete set or the new one). Any failure leaves the previous
 *   set untouched.
 *
 * Deletions (each guarded by [deleteExact]: fixed name, canonical parent check, never `filesDir`
 * or `cacheDir` themselves):
 * - `cacheDir/lsfg-import/Lossless.dll.tmp` (file);
 * - `cacheDir/lsfg-import` (only if empty, non-recursive);
 * - `filesDir/lsfg/spirv.staging` (recursive);
 * - `filesDir/lsfg/spirv` (recursive, only right before the staging rename).
 *
 * Native code mapping for [FgNative.extractShaders] (see [mapExtractCode]):
 * DLL_UNREADABLE (-20) -> UNREADABLE; MISSING_SPIRV (-21) -> MISSING_SHADERS; BINDING_REWRITE
 * (-22) and WRITE_FAILED (-23) -> WRITE_FAILED; INVALID_ARG (-11) and any other negative code ->
 * WRITE_FAILED (the paths are built here, so an argument rejection means the private storage is
 * not usable). NOT_INITIALIZED (-1) is unreachable: [FgNative.isAvailable] is checked first and
 * yields UNAVAILABLE. A count >= 0 whose staging set fails the probe -> MISSING_SHADERS.
 *
 * Construction: the public constructor takes a [Context] (uses `filesDir`, `cacheDir` and the
 * `ContentResolver`). The internal one injects the directories, the stream opener and the native
 * entry points so JVM tests can run against temporary directories (rule 5: tests never write to
 * real user directories). Installs are serialized by a [Mutex] and run on [Dispatchers.IO].
 * Callers must not install while a pipeline is running on the current set.
 */
class ShaderInstaller internal constructor(
  private val filesDir: File,
  private val cacheDir: File,
  private val openUri: (Uri) -> InputStream?,
  private val available: () -> Boolean = FgNative::isAvailable,
  private val extract: (File, File) -> Int = FgNative::extractShaders,
  private val probe: (File) -> Boolean = FgNative::probeShaders,
) {
  constructor(context: Context) : this(
    filesDir = context.filesDir,
    cacheDir = context.cacheDir,
    openUri = context.applicationContext.contentResolver::openInputStream,
  )

  private val mutex = Mutex()

  /** `filesDir/lsfg`. */
  private val lsfgDir: File get() = File(filesDir, LSFG_DIR)

  /** `filesDir/lsfg/spirv.staging`. */
  private val stagingDir: File get() = File(lsfgDir, STAGING_DIR)

  /** `cacheDir/lsfg-import`. */
  private val importDir: File get() = File(cacheDir, IMPORT_DIR)

  /** `cacheDir/lsfg-import/Lossless.dll.tmp`. */
  private val tmpDll: File get() = File(importDir, TMP_DLL)

  /** Installed shader set: `filesDir/lsfg/spirv` (the `shaderDir` passed to [FgConfig]). */
  fun shaderDir(): File = File(lsfgDir, SPIRV_DIR)

  /** True when the native library is loaded and [shaderDir] holds the complete FP32 set. */
  fun isReady(): Boolean = available() && probe(shaderDir())

  /** The manifest of the installed set, or null when it does not exist or cannot be parsed. */
  fun manifest(): ShaderManifest? {
    val file = File(shaderDir(), MANIFEST_NAME)
    return runCatching {
      if (!file.isFile) return null
      json.decodeFromString(ShaderManifest.serializer(), file.readText())
    }.getOrNull()
  }

  /** Imports the DLL behind [uri]; see the class KDoc. */
  suspend fun install(uri: Uri): InstallResult = installFrom { openUri(uri) }

  /** [install] with an arbitrary stream opener (tests). */
  internal suspend fun installFrom(open: () -> InputStream?): InstallResult =
    withContext(Dispatchers.IO) { mutex.withLock { installLocked(open) } }

  private fun installLocked(open: () -> InputStream?): InstallResult {
    if (!available()) return InstallResult.UNAVAILABLE
    try {
      if (!ensureDir(importDir) || !ensureDir(lsfgDir)) return InstallResult.WRITE_FAILED
      // Leftovers of an interrupted run.
      if (!deleteExact(tmpDll, importDir, TMP_DLL, recursive = false)) return InstallResult.WRITE_FAILED
      if (!deleteExact(stagingDir, lsfgDir, STAGING_DIR, recursive = true)) return InstallResult.WRITE_FAILED

      val copied = copyBounded(open, tmpDll)
      copied.failure?.let { return it }

      val code = extract(tmpDll, stagingDir)
      if (code < 0) return mapExtractCode(code)
      if (!probe(stagingDir)) return InstallResult.MISSING_SHADERS

      val manifest = ShaderManifest(
        sha256 = copied.sha256,
        fp32Count = code,
        fp16Present = File(stagingDir, FP16_DIR).listFiles()?.any { it.isFile } == true,
        importedAt = Instant.now().toString(),
      )
      File(stagingDir, MANIFEST_NAME).writeText(json.encodeToString(ShaderManifest.serializer(), manifest))

      val target = shaderDir()
      if (!deleteExact(target, lsfgDir, SPIRV_DIR, recursive = true)) return InstallResult.WRITE_FAILED
      if (!stagingDir.renameTo(target)) return InstallResult.WRITE_FAILED
      return InstallResult.IMPORTED
    } catch (_: IOException) {
      return InstallResult.WRITE_FAILED
    } catch (_: SecurityException) {
      return InstallResult.WRITE_FAILED
    } finally {
      // Fact 11: the DLL copy never survives the call; neither does a partial staging set.
      deleteExact(tmpDll, importDir, TMP_DLL, recursive = false)
      deleteExact(stagingDir, lsfgDir, STAGING_DIR, recursive = true)
      deleteExact(importDir, cacheDir, IMPORT_DIR, recursive = false)
    }
  }

  /**
   * Tatakai (Settings › «Eliminar»): removes the installed set (`filesDir/lsfg/spirv`, through [deleteExact]) and any staging
   * leftover. True when no set remains. Same rule as [install]: never while a pipeline runs on the set.
   */
  suspend fun remove(): Boolean =
    withContext(Dispatchers.IO) {
      mutex.withLock {
        deleteExact(stagingDir, lsfgDir, STAGING_DIR, recursive = true)
        deleteExact(shaderDir(), lsfgDir, SPIRV_DIR, recursive = true)
      }
    }

  private class Copied(val failure: InstallResult?, val sha256: String)

  /**
   * Copies [open]'s stream into [out] (at most [LSFG_DLL_MAX_BYTES]) hashing it with SHA-256.
   * Open/read errors and an empty stream are UNREADABLE; write errors are WRITE_FAILED.
   */
  private fun copyBounded(open: () -> InputStream?, out: File): Copied {
    val input = try {
      open()
    } catch (_: Exception) {
      null
    } ?: return Copied(InstallResult.UNREADABLE, "")
    input.use { source ->
      val digest = MessageDigest.getInstance("SHA-256")
      val sink = try {
        FileOutputStream(out)
      } catch (_: IOException) {
        return Copied(InstallResult.WRITE_FAILED, "")
      }
      sink.use { output ->
        val buffer = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
          val read = try {
            source.read(buffer)
          } catch (_: IOException) {
            return Copied(InstallResult.UNREADABLE, "")
          }
          if (read < 0) break
          total += read
          if (total > LSFG_DLL_MAX_BYTES) return Copied(InstallResult.TOO_LARGE, "")
          digest.update(buffer, 0, read)
          try {
            output.write(buffer, 0, read)
          } catch (_: IOException) {
            return Copied(InstallResult.WRITE_FAILED, "")
          }
        }
        if (total == 0L) return Copied(InstallResult.UNREADABLE, "")
      }
      return Copied(null, digest.digest().joinToString("") { "%02x".format(it) })
    }
  }

  private fun ensureDir(dir: File): Boolean = dir.isDirectory || dir.mkdirs()

  /**
   * Deletes [target] only when its name is exactly [name], its canonical parent is exactly
   * [parent]'s canonical path and it is neither `filesDir` nor `cacheDir`. A symlink pointing
   * elsewhere fails the canonical check and is refused. Returns true when [target] no longer exists.
   */
  private fun deleteExact(target: File, parent: File, name: String, recursive: Boolean): Boolean =
    runCatching {
      if (name.isEmpty() || target.name != name) return@runCatching false
      val canonical = target.canonicalFile
      if (canonical.parentFile != parent.canonicalFile) return@runCatching false
      if (canonical == filesDir.canonicalFile || canonical == cacheDir.canonicalFile) return@runCatching false
      if (!target.exists()) return@runCatching true
      if (recursive) target.deleteRecursively() else target.delete()
      !target.exists()
    }.getOrDefault(false)

  internal companion object {
    const val LSFG_DIR = "lsfg"
    const val SPIRV_DIR = "spirv"
    const val STAGING_DIR = "spirv.staging"
    const val FP16_DIR = "fp16"
    const val IMPORT_DIR = "lsfg-import"
    const val TMP_DLL = "Lossless.dll.tmp"
    const val MANIFEST_NAME = "manifest.json"

    private val json = Json { ignoreUnknownKeys = true }

    /** Maps a negative [FgNative.extractShaders] code; see the class KDoc. */
    fun mapExtractCode(code: Int): InstallResult = when (FgResult.fromCode(code)) {
      FgResult.DLL_UNREADABLE -> InstallResult.UNREADABLE
      FgResult.MISSING_SPIRV -> InstallResult.MISSING_SHADERS
      FgResult.BINDING_REWRITE, FgResult.WRITE_FAILED -> InstallResult.WRITE_FAILED
      else -> InstallResult.WRITE_FAILED
    }
  }
}
