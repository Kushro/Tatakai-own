// Integrated extension server client (docs/redesign/13-integrated-server-contract.md §3.10, owner w6).
// Android library because the connection store seals tokens with an AndroidKeyStore key; the HTTP client, host guard, deep link
// parser and candidate mapping are plain JVM code covered by unit tests.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    // ExternalCandidateSource (merge point of ExtensionRepositoryImpl.resolveCandidates) and Candidates.mapCandidates
    implementation(projects.core.extensions)
    // KeyCipher / AesGcmKeyCipher: same at-rest sealing as the Link pairing
    implementation(projects.core.link)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
