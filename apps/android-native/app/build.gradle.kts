// :app (m3e/android-shell). parity wave A, 2026-11-09 amendment (plan 14 §0.1 #9): the hosted-backend BuildConfig injection (E8) and the
// `checkTatakaiConfig` task are gone with the own login. The only injected values are the optional OAuth client ids of the list providers.
import java.util.Properties

plugins {
    id("tatakai.android.application")
    id("tatakai.android.compose")
}

// Config injection. Values come from (in order): local.properties, then environment variables. Nothing is committed and nothing is
// required: without a client id, Settings › Listas y cuentas shows "Vincular" disabled with the reason.
val localProps = Properties().apply {
    rootProject.file("local.properties").takeIf { it.isFile }?.inputStream()?.use { load(it) }
}
fun cfg(name: String): String = (localProps.getProperty(name) ?: System.getenv(name) ?: "").trim()

// parity wave A (docs/redesign/15 §3.3): OAuth client ids of the list providers (public ids; one per platform because AniList allows one
// redirect per client).
val anilistClientId = cfg("TATAKAI_ANILIST_CLIENT_ID")
val malClientId = cfg("TATAKAI_MAL_CLIENT_ID")
fun String.asBuildConfigString() = "\"" + replace("\\", "\\\\").replace("\"", "\\\"") + "\""

// Release signing (2026-11-10): TATAKAI_SIGNING = path of a keystore .properties file (storeFile, storeType, keyAlias,
// storePassword, keyPassword), e.g. KKUP/tatakai.keystore.properties locally or one CI writes from repo secrets. When its
// storeFile does not exist on this machine (a Windows path), the keystore next to the .properties file with the same base
// name is used. Without TATAKAI_SIGNING the release APK is unsigned (CI refuses to publish it).
val signingProps: Properties? = cfg("TATAKAI_SIGNING").takeIf { it.isNotEmpty() }?.let { path ->
    val f = file(path)
    require(f.isFile) { "TATAKAI_SIGNING points at a missing file: $path" }
    Properties().apply { f.inputStream().use { load(it) } }.also { it.setProperty("__dir", f.parentFile.path); it.setProperty("__base", f.name.removeSuffix(".properties")) }
}
// Versions come from the release tag in CI (TATAKAI_VERSION_NAME / TATAKAI_VERSION_CODE); local builds are "-dev".
val versionNameCfg = cfg("TATAKAI_VERSION_NAME").ifEmpty { "0.1.0-dev" }
val versionCodeCfg = cfg("TATAKAI_VERSION_CODE").toIntOrNull() ?: 1

android {
    namespace = "app.tatakai.next"

    defaultConfig {
        applicationId = "app.tatakai.own.next"
        versionCode = versionCodeCfg
        versionName = versionNameCfg
        buildConfigField("String", "TATAKAI_ANILIST_CLIENT_ID", anilistClientId.asBuildConfigString())
        buildConfigField("String", "TATAKAI_MAL_CLIENT_ID", malClientId.asBuildConfigString())
    }
    buildFeatures { buildConfig = true }
    // Release CI (TATAKAI_ABI_SPLITS=true): one APK per ABI (arm64 ~70 MB instead of ~180 MB) plus a universal one;
    // same versionCode for all (sideloaded, never Play). Local builds keep the single universal APK.
    splits {
        abi {
            isEnable = cfg("TATAKAI_ABI_SPLITS") == "true"
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86_64")
            isUniversalApk = true
        }
    }
    signingProps?.let { p ->
        signingConfigs {
            create("release") {
                val declared = p.getProperty("storeFile")?.let { file(it) }
                storeFile = declared?.takeIf { it.isFile } ?: File(p.getProperty("__dir"), p.getProperty("__base"))
                storeType = p.getProperty("storeType") ?: "PKCS12"
                keyAlias = p.getProperty("keyAlias")
                storePassword = p.getProperty("storePassword")
                keyPassword = p.getProperty("keyPassword")
            }
        }
        buildTypes { getByName("release") { signingConfig = signingConfigs.getByName("release") } }
    }
    // The check*AarMetadata tasks are disabled repo-wide (see root build.gradle.kts), but release lintVital
    // pulls in each library's bundleReleaseLocalLintAar, which requires that task's output directory to exist.
    // Skip lintVital on assembleRelease; run `./gradlew lint` explicitly when needed.
    lint { checkReleaseBuilds = false }
}

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.designsystem)
    implementation(projects.core.ui)
    implementation(projects.core.network)
    implementation(projects.core.metadata)
    implementation(projects.core.database)
    implementation(projects.core.data)
    implementation(projects.core.extensions)
    implementation(projects.core.extensionsQuickjs)
    implementation(projects.core.player)
    implementation(projects.core.link)
    implementation(projects.feature.home)
    implementation(projects.feature.search)
    implementation(projects.feature.details)
    implementation(projects.feature.library)
    implementation(projects.feature.settings)
    implementation(projects.feature.extensions)
    implementation(projects.feature.link)
    implementation(projects.feature.player)
    // android wave (12-android-wave-contract.md)
    implementation(projects.core.release)
    implementation(projects.core.torrent)
    implementation(projects.core.playerMpv)
    implementation(projects.feature.torrents)
    implementation(projects.feature.you)
    // integrated extension server client (docs/redesign/13 §3.10)
    implementation(projects.core.extensionServer)
    // parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.8): r (discover, download, downloads), s (manga, novel), t (social)
    implementation(projects.core.discover)
    implementation(projects.core.download)
    implementation(projects.core.manga)
    implementation(projects.core.novel)
    implementation(projects.core.social)
    implementation(projects.feature.downloads)
    implementation(projects.feature.manga)
    implementation(projects.feature.novel)
    implementation(projects.feature.social)

    implementation(libs.bundles.compose.core)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.viewmodel.navigation3)
    implementation(libs.navigation3.runtime)
    implementation(libs.navigation3.ui)
    implementation(libs.material3.adaptive)
    implementation(libs.material3.adaptive.layout)
    implementation(libs.material3.adaptive.navigation)
    implementation(libs.material3.adaptive.navigation3)
    implementation(libs.compose.material3.adaptive.navigation.suite)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.koin.bom))
    implementation(libs.bundles.koin.android)
}

// android wave (worker b): Settings > About > Licenses reads THIRD_PARTY_NOTICES.md from the APK assets
// (`licenses/THIRD_PARTY_NOTICES.md`); the OFL font texts come from :core:designsystem's assets. Wired through the AGP
// variant API so every assets consumer depends on the copy task.
abstract class CopyThirdPartyNotices : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val notices: RegularFileProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun copy() {
        val target = outputDir.get().asFile.resolve("licenses")
        target.mkdirs()
        notices.get().asFile.copyTo(target.resolve("THIRD_PARTY_NOTICES.md"), overwrite = true)
    }
}

val copyThirdPartyNotices = tasks.register<CopyThirdPartyNotices>("copyThirdPartyNotices") {
    notices.set(rootProject.layout.projectDirectory.file("THIRD_PARTY_NOTICES.md"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.assets?.addGeneratedSourceDirectory(copyThirdPartyNotices, CopyThirdPartyNotices::outputDir)
    }
}
