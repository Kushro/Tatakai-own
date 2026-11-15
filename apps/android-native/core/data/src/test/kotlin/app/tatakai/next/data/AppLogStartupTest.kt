// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.AppLog
import app.tatakai.next.model.LogSink
import org.junit.After
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import org.koin.core.annotation.KoinInternalApi
import org.koin.dsl.koinApplication
import java.io.File
import kotlin.test.Test
import kotlin.test.assertTrue

/** BUG-145: the AppLog sink is installed when Koin starts, not when Settings › Acerca de first asks for the LogRepository. */
class AppLogStartupTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val previous: LogSink = AppLog.sink

    @After
    fun restore() {
        AppLog.sink = previous
    }

    @Test
    fun startingKoinInstallsTheFileSinkWithoutAnyoneAskingForIt() {
        AppLog.sink = LogSink { _, _, _, _ -> }
        val dir = tmp.newFolder("logs")
        val app = koinApplication { modules(logModule { dir }) }
        try {
            // nobody resolved LogRepository: the eager single did
            assertTrue(AppLog.sink is AppLogStore, "AppLog.sink is still the no-op after startKoin")
            AppLog.i("metadata", "media anilist:154587: anilist cooling down → kitsu (fallback answer)")
            val file = File(dir, AppLogStore.FILE)
            val deadline = System.currentTimeMillis() + 5_000
            while (!(file.isFile && file.readText().contains("fallback answer")) && System.currentTimeMillis() < deadline) Thread.sleep(20)
            assertTrue(file.readText().contains("I/metadata: media anilist:154587"), file.readText())
        } finally {
            app.close()
        }
    }

    @OptIn(KoinInternalApi::class)
    @Test
    fun dataModuleCarriesTheEagerLogModule() {
        assertTrue(appLogModule in dataModule.includedModules)
        assertTrue(appLogModule.eagerInstances.isNotEmpty(), "the LogRepository single is not createdAtStart")
    }
}
