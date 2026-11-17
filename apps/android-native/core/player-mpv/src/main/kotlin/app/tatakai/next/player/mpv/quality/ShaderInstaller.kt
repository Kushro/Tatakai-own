// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import java.io.File

// 0.4.0 (worker A): turns the planner's [ShaderSpec]s into files mpv can load. Replaces 0.3.x `ShaderPresets.install` (one folder per
// preset): files now live once in a folder keyed by the APK build, so an app update always installs fresh copies (Enigmata's E28 fix:
// a stale copy from a previous APK must never win) and old folders are removed. FP16 Anime4K copies (Enigmata's
// Anime4KShaderOptimizer) live in their own sub-folder, so switching precision changes the path and mpv reloads the shader.

/** Resolves a chain to absolute paths (installing what is missing). Null when a file could not be installed (the chain then stays off). */
fun interface ShaderResolver {
    fun resolve(specs: List<ShaderSpec>, fp16: Boolean): List<String>?
}

object ShaderNames {
    const val FP16_DIR = "fp16-v${Anime4KShaderOptimizer.VERSION}"
    const val GENERATED_DIR = "gen"

    /** Whether [asset] gets an FP16 copy (Enigmata's allowlist: `Anime4K_*` only; FSR / FSRCNNX / SSim / CAS stay verbatim). */
    fun fp16Eligible(asset: String): Boolean = Anime4KShaderOptimizer.shouldOptimize(asset)

    /**
     * Relative path of each pass inside the build folder, in chain order. A file used twice in one chain (Anime4K FAST = S/S) gets a
     * second copy (`name__1.glsl`), as 0.3.x did with its index prefixes, so each pass is its own file whatever mpv does with a
     * repeated entry of the `glsl-shaders` list.
     */
    fun relativePaths(specs: List<ShaderSpec>, fp16: Boolean): List<String> {
        val seen = HashMap<String, Int>()
        return specs.map { spec ->
            val base = when {
                spec.transform == ShaderTransform.CAS -> "$GENERATED_DIR/${ShaderSources.casFileName(spec.casSharpening)}"
                fp16 && fp16Eligible(spec.asset) -> "$FP16_DIR/${spec.asset}"
                else -> spec.asset
            }
            val n = seen.getOrDefault(base, 0)
            seen[base] = n + 1
            if (n == 0) base else base.removeSuffix(".glsl") + "__$n.glsl"
        }
    }

    /** Whether a chain resolved with [fp16] actually uses an FP16 copy (beyond the cheap Clamp pre-pass, Enigmata's rule). */
    fun usesFp16(specs: List<ShaderSpec>, fp16: Boolean): Boolean =
        fp16 && specs.any { it.transform == ShaderTransform.NONE && fp16Eligible(it.asset) && it.asset != ShaderCatalog.CLAMP }
}

/** [ShaderResolver] over the APK assets (`assets/shaders/`), installed under `filesDir/shaders/<build>/`. */
class AssetShaderInstaller(context: Context) : ShaderResolver {
    private val appContext = context.applicationContext
    private val root = File(appContext.filesDir, "shaders")
    private val dir: File by lazy { File(root, buildKey()) }

    @Volatile
    private var cleaned = false

    @Synchronized
    override fun resolve(specs: List<ShaderSpec>, fp16: Boolean): List<String>? {
        if (specs.isEmpty()) return emptyList()
        cleanOldBuilds()
        val names = ShaderNames.relativePaths(specs, fp16)
        return runCatching {
            specs.zip(names).map { (spec, name) ->
                val target = File(dir, name)
                if (!target.isFile || target.length() == 0L) write(target, content(spec, name))
                target.absolutePath
            }
        }.onFailure { android.util.Log.w("mpv", "shader install failed: ${it.message}") }.getOrNull()
    }

    private fun content(spec: ShaderSpec, name: String): ByteArray {
        val raw = appContext.assets.open("${ShaderCatalog.ASSET_DIR}/${spec.asset}").use { it.readBytes() }
        return when {
            spec.transform == ShaderTransform.CAS ->
                ShaderSources.casVariant(raw.toString(Charsets.UTF_8), spec.casSharpening).toByteArray(Charsets.UTF_8)
            name.startsWith("${ShaderNames.FP16_DIR}/") ->
                // A rewrite that throws falls back to the verbatim text (never an empty chain).
                runCatching { Anime4KShaderOptimizer.optimize(raw.toString(Charsets.UTF_8)).toByteArray(Charsets.UTF_8) }.getOrDefault(raw)
            else -> raw
        }
    }

    /** Write-then-rename, so mpv never reads a half-written shader. */
    private fun write(target: File, bytes: ByteArray) {
        target.parentFile?.mkdirs()
        val tmp = File(target.parentFile, "${target.name}.tmp")
        tmp.writeBytes(bytes)
        if (!tmp.renameTo(target)) {
            target.writeBytes(bytes)
            tmp.delete()
        }
    }

    /** Removes the folders of previous builds and the 0.3.x per-preset folders (`shaders/anime4k-a/`...), once per process. */
    private fun cleanOldBuilds() {
        if (cleaned) return
        cleaned = true
        val keep = dir.name
        root.listFiles()?.filter { it.isDirectory && it.name != keep }?.forEach { runCatching { it.deleteRecursively() } }
    }

    /** `b<versionCode>-<lastUpdateTime>`: changes with every APK install, also debug reinstalls of the same version. */
    private fun buildKey(): String = runCatching {
        val pm = appContext.packageManager
        val info = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            pm.getPackageInfo(appContext.packageName, PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageInfo(appContext.packageName, 0)
        }
        val code = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) info.longVersionCode else {
            @Suppress("DEPRECATION")
            info.versionCode.toLong()
        }
        "b$code-${info.lastUpdateTime}"
    }.getOrDefault("b0")
}
