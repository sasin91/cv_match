// JobFinder for Android: the phone and tablet app that pairs with JobFinder on the computer.
pluginManagement {
    repositories {
        google()
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

rootProject.name = "jobfinder-android"

// What the app and the computer send each other, and the client that sends it: plain Kotlin,
// so its tests run on any JVM.
include(":protocol")
include(":app")
