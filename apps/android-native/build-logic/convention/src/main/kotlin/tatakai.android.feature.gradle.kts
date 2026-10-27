// feature module = library + compose + the common UI dependencies (core:ui, core:designsystem, core:model, Koin, lifecycle).
plugins {
    id("tatakai.android.library")
    id("tatakai.android.compose")
}

dependencies {
    add("implementation", project(":core:model"))
    add("implementation", project(":core:designsystem"))
    add("implementation", project(":core:ui"))
    add("implementation", tatakaiCatalog.bundle("compose-core"))
    add("implementation", platform(tatakaiCatalog.lib("koin-bom")))
    add("implementation", tatakaiCatalog.bundle("koin-android"))
    add("implementation", tatakaiCatalog.lib("androidx-lifecycle-runtime-compose"))
    add("implementation", tatakaiCatalog.lib("androidx-lifecycle-viewmodel-compose"))
    add("implementation", tatakaiCatalog.lib("kotlinx-coroutines-android"))
}
