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
