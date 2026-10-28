// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.AppException
import app.tatakai.next.model.ExtensionCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import java.io.File
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Install / enable / remove against a temp directory (never a real user directory) with an engine that must not be used. */
class ExtensionRepositoryImplTest {
    private val engine = object : JsEngine {
        override suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession = error("engine must not run here")
    }

    private fun fixtures(): File =
        generateSequence(File(System.getProperty("user.dir")).absoluteFile) { it.parentFile }
            .map { File(it, "contracts/fixtures/extensions/packed") }.first { it.isDirectory }

    private fun repo(root: File) = ExtensionRepositoryImpl(
        rootDir = root,
        openUri = { File(it).inputStream() },
        host = ExtensionHost(engine, { error("no host") }),
        io = Dispatchers.Unconfined,
    )

    @Test
    fun installEnableRemoveLifecycle() = runBlocking {
        val root = Files.createTempDirectory("tatakai-ext-test").toFile()
        try {
            val repo = repo(root)
            val full = repo.installFromUri(File(fixtures(), "synthetic-full.kai").path).getOrThrow()
            assertEquals(ExtensionCompat.FULL, full.compat)
            assertTrue(full.enabled)
            val fs = repo.installFromUri(File(fixtures(), "synthetic-fs.kai").path).getOrThrow()
            assertEquals(ExtensionCompat.UNSUPPORTED, fs.compat)
            assertFalse(fs.enabled)

            repo.setEnabled("synthetic-fs", true) // forced to stay disabled
            assertFalse(repo.installed.value.first { it.id == "synthetic-fs" }.enabled)
            repo.setEnabled("synthetic-full", false)
            assertFalse(repo.installed.value.first { it.id == "synthetic-full" }.enabled)
            assertTrue(File(root, "synthetic-full/bundle.js").isFile)

            // a second repository over the same directory sees the persisted state
            val again = repo(root)
            again.installed.first { it.size == 2 }
            assertFalse(again.installed.value.first { it.id == "synthetic-full" }.enabled)

            repo.remove("synthetic-full")
            assertEquals(listOf("synthetic-fs"), repo.installed.value.map { it.id })
            assertFalse(File(root, "synthetic-full").exists())
        } finally {
            root.deleteRecursively() // temp dir created above, literal path from createTempDirectory
        }
    }

    @Test
    fun invalidPackageFailsWithExtensionError() = runBlocking {
        val root = Files.createTempDirectory("tatakai-ext-test").toFile()
        try {
            val bad = File(root, "bad.kai").apply { writeText("nope") }
            val r = repo(File(root, "installed")).installFromUri(bad.path)
            assertTrue(r.isFailure)
            assertTrue(r.exceptionOrNull() is AppException)
        } finally {
            root.deleteRecursively()
        }
    }

}
