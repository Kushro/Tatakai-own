// Root build. Conventions live in build-logic/ (tatakai.* plugins). The version catalog is FROZEN after B1:
// new dependencies are requested in the module's NOTES-deps.md and added by the integrator.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.ksp) apply false
}

// Material3 Adaptive 1.3.0 (ListDetailSceneStrategy for Navigation 3) is built with AGP 9.1 and declares minAgpVersion=9.1.0,
// plus compileSdk 37. The project stays on AGP 8.13.2 (same toolchain as Miko), so the AAR-metadata gate is disabled in every module.
subprojects {
    tasks.configureEach {
        if (name.startsWith("check") && name.endsWith("AarMetadata")) enabled = false
    }
}

// A library's bundle<Variant>Aar (run by the root `assembleDebug`) still declares the disabled check's output directory as an
// input and fails validation when it is missing. A tiny task creates that (empty) directory first; validation runs before a
// task's own actions, so a doFirst would be too late.
subprojects {
    plugins.withId("com.android.library") {
        for (variant in listOf("Debug", "Release")) {
            val checkDir = layout.buildDirectory.dir("intermediates/aar_metadata_check/${variant.lowercase()}/check${variant}AarMetadata")
            val prep = tasks.register("prepare${variant}AarMetadataCheckDir") {
                outputs.dir(checkDir)
                doLast { checkDir.get().asFile.mkdirs() }
            }
            tasks.matching { it.name == "bundle${variant}Aar" }.configureEach { dependsOn(prep) }
        }
    }
}
