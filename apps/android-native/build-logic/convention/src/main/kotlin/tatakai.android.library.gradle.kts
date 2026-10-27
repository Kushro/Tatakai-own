import com.android.build.api.dsl.LibraryExtension

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

extensions.configure<LibraryExtension> {
    namespace = project.tatakaiNamespace()
    configureAndroidCommon(this)
}

dependencies {
    add("testImplementation", tatakaiCatalog.bundle("jvm-test"))
}
