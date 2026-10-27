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
include(":core:supabase")
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
