import com.android.build.api.dsl.ApplicationExtension

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

extensions.configure<ApplicationExtension> {
    defaultConfig.targetSdk = TatakaiAndroid.TARGET_SDK
    configureAndroidCommon(this)
}

dependencies {
    add("testImplementation", tatakaiCatalog.bundle("jvm-test"))
}

// Material3 Adaptive 1.3.0 (ListDetailSceneStrategy for Navigation 3) is built with AGP 9.1 and declares minAgpVersion=9.1.0.
// The project stays on AGP 8.13.2 (same toolchain as Miko), so the metadata gate is disabled; compileSdk 37 is required by those AARs.
tasks.configureEach {
    if (name.startsWith("check") && name.endsWith("AarMetadata")) enabled = false
}
