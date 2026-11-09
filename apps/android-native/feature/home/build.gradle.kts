plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    // parity wave B (worker r): TrendingFeed (Top 10 windows), same as :feature:search
    implementation(projects.core.metadata)
    // notification permission request of the calendar / "Seguir" (rememberLauncherForActivityResult)
    implementation(libs.androidx.activity.compose)
}
