import com.android.build.api.dsl.CommonExtension
import org.gradle.api.JavaVersion
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalog
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.gradle.api.tasks.compile.JavaCompile
import org.gradle.kotlin.dsl.getByType
import org.gradle.kotlin.dsl.withType
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

/** Shared constants. Keep in sync with apps/README.md. */
object TatakaiAndroid {
    const val COMPILE_SDK = 37
    const val TARGET_SDK = 36
    const val MIN_SDK = 26
    val JAVA = JavaVersion.VERSION_17
    val JVM_TARGET = JvmTarget.JVM_17
}

val Project.tatakaiCatalog: VersionCatalog
    get() = extensions.getByType<VersionCatalogsExtension>().named("libs")

fun VersionCatalog.lib(alias: String) = findLibrary(alias).get()
fun VersionCatalog.bundle(alias: String) = findBundle(alias).get()

/**
 * Android namespace derived from the Gradle path:
 *   :core:model        -> app.tatakai.next.model
 *   :core:extensions-quickjs -> app.tatakai.next.extensions_quickjs
 *   :feature:home      -> app.tatakai.next.feature.home
 *   :app               -> app.tatakai.next   (set explicitly in :app)
 */
fun Project.tatakaiNamespace(): String {
    val segments = path.trim(':').split(':').map { it.replace('-', '_') }
    return when (segments.first()) {
        "core" -> "app.tatakai.next." + segments.drop(1).joinToString(".")
        else -> "app.tatakai.next." + segments.joinToString(".")
    }
}

fun Project.configureAndroidCommon(ext: CommonExtension<*, *, *, *, *, *>) {
    ext.apply {
        compileSdk = TatakaiAndroid.COMPILE_SDK
        defaultConfig.minSdk = TatakaiAndroid.MIN_SDK
        compileOptions {
            sourceCompatibility = TatakaiAndroid.JAVA
            targetCompatibility = TatakaiAndroid.JAVA
        }
        testOptions.unitTests.isReturnDefaultValues = true
    }
    tasks.withType<KotlinCompile>().configureEach {
        compilerOptions {
            jvmTarget.set(TatakaiAndroid.JVM_TARGET)
            freeCompilerArgs.add("-opt-in=kotlin.RequiresOptIn")
        }
    }
}

fun Project.configureJvmCommon() {
    tasks.withType<KotlinCompile>().configureEach {
        compilerOptions {
            jvmTarget.set(TatakaiAndroid.JVM_TARGET)
        }
    }
    tasks.withType<JavaCompile>().configureEach {
        options.release.set(17)
    }
}
