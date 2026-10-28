import com.android.build.api.dsl.CommonExtension

// Apply AFTER tatakai.android.library / tatakai.android.application.
plugins {
    id("org.jetbrains.kotlin.plugin.compose")
}

extensions.configure<CommonExtension<*, *, *, *, *, *>>("android") {
    buildFeatures.compose = true
}

dependencies {
    add("implementation", platform(tatakaiCatalog.lib("compose-bom")))
    add("debugImplementation", tatakaiCatalog.lib("compose-ui-tooling"))
}
