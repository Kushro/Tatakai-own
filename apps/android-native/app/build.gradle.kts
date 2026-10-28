// Placeholder from B1 (m3e/foundation). B2 (m3e/android-shell) owns :app and may overwrite everything below
// EXCEPT the Supabase BuildConfig injection (E8) which must be preserved.
import java.util.Properties

plugins {
    id("tatakai.android.application")
    id("tatakai.android.compose")
}

// E8 - config injection. Values come from (in order): local.properties, then environment variables.
// Nothing is committed. The build FAILS (task checkTatakaiConfig, wired to preBuild) when they are missing;
// `./gradlew help`, `dependencies` and other module builds are unaffected.
val localProps = Properties().apply {
    rootProject.file("local.properties").takeIf { it.isFile }?.inputStream()?.use { load(it) }
}
fun cfg(name: String): String = (localProps.getProperty(name) ?: System.getenv(name) ?: "").trim()

val supabaseUrl = cfg("TATAKAI_SUPABASE_URL")
val supabaseAnonKey = cfg("TATAKAI_SUPABASE_ANON_KEY")
fun String.asBuildConfigString() = "\"" + replace("\\", "\\\\").replace("\"", "\\\"") + "\""

android {
    namespace = "app.tatakai.next"

    defaultConfig {
        applicationId = "app.tatakai.own.next"
        versionCode = 1
        versionName = "0.1.0-m0"
        buildConfigField("String", "TATAKAI_SUPABASE_URL", supabaseUrl.asBuildConfigString())
        buildConfigField("String", "TATAKAI_SUPABASE_ANON_KEY", supabaseAnonKey.asBuildConfigString())
    }
    buildFeatures { buildConfig = true }
}

val checkTatakaiConfig by tasks.registering {
    group = "verification"
    description = "Fails if TATAKAI_SUPABASE_URL / TATAKAI_SUPABASE_ANON_KEY are not provided (local.properties or env)."
    val url = supabaseUrl
    val key = supabaseAnonKey
    doLast {
        val missing = listOfNotNull(
            "TATAKAI_SUPABASE_URL".takeIf { url.isBlank() },
            "TATAKAI_SUPABASE_ANON_KEY".takeIf { key.isBlank() },
        )
        check(missing.isEmpty()) {
            "Missing ${missing.joinToString()}: set them in apps/android-native/local.properties " +
                "(see local.properties.example) or as environment variables."
        }
    }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(checkTatakaiConfig) }

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.designsystem)
    implementation(projects.core.ui)
    implementation(projects.core.network)
    implementation(projects.core.metadata)
    implementation(projects.core.database)
    implementation(projects.core.supabase)
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
