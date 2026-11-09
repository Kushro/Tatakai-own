pluginManagement {
    includeBuild("build-logic")
    repositories {
        google {
            content {
                includeGroupByRegex("com.android.*")
                includeGroupByRegex("com.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "tatakai-android-native"
enableFeaturePreview("TYPESAFE_PROJECT_ACCESSORS")

include(":app")

// core
include(":core:designsystem")
include(":core:ui")
include(":core:model")
include(":core:network")
include(":core:metadata")
include(":core:database")
include(":core:data")
include(":core:extensions")
include(":core:extensions-quickjs")
include(":core:player")
include(":core:link")

// feature
include(":feature:home")
include(":feature:search")
include(":feature:details")
include(":feature:library")
include(":feature:settings")
include(":feature:extensions")
include(":feature:link")
include(":feature:player")

// android wave (docs/redesign/12-android-wave-contract.md)
include(":core:release")
include(":core:torrent")
include(":core:player-mpv")
include(":feature:torrents")
include(":feature:you")

// integrated extension server client (docs/redesign/13-integrated-server-contract.md §3.10)
include(":core:extension-server")

// parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.8): new modules of workers r / s / t (created by the contract)
include(":core:discover")
include(":core:download")
include(":core:manga")
include(":core:novel")
include(":core:social")
include(":feature:downloads")
include(":feature:manga")
include(":feature:novel")
include(":feature:social")
