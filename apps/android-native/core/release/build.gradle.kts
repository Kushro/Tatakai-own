// android wave contract (docs/redesign/12-android-wave-contract.md), owner: worker a.
// Pure JVM port of apps/desktop-native/crates/tk-core/src/{release.rs,sources.rs}: parseRelease, isVideoPath, pickEpisodeFile and the
// source ranking (score/rank/updateReliability). Tests read contracts/fixtures/release/{names,packs}.json and contracts/fixtures/sources.
plugins { id("tatakai.jvm.library") }

dependencies {
    api(projects.core.model)
    testImplementation(libs.kotlinx.serialization.json)
}
