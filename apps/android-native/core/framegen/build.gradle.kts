// SPDX-License-Identifier: AGPL-3.0-or-later
// Tatakai Next 0.4.0 (worker B): frame generation on the phone (LSFG, ported from Enigmata R13). Kotlin controller + policy and
// the native library libtatakai_fg.so (CMake, arm64-v8a / x86_64 only; the 32-bit ABIs build nothing and FG reports "ABI").
// :core:player-mpv routes MPV's output through it (MpvFrameGenBridge); :feature:settings shows the Lossless.dll import.
plugins {
    id("tatakai.android.library")
}

android {
    // The only NDK installed on the build hosts (same as Enigmata); AGP's default NDK is a different one.
    ndkVersion = "28.2.13676358"
    defaultConfig {
        consumerProguardFiles("consumer-rules.pro")
        externalNativeBuild {
            cmake {
                // Every ABI the libmpv AAR ships: the 32-bit ones configure an empty project (no FG library).
                abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
            }
        }
    }
    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }
}

dependencies {
    api(projects.core.model)
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
