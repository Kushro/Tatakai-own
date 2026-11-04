// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next

import android.app.Activity
import android.app.LocaleManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import android.os.LocaleList
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.SettingsRepository
import org.koin.core.context.GlobalContext
import java.util.Locale

/**
 * Per-app language (Settings › Apariencia › Idioma, 12 §4 b.1): system / en / es.
 *
 * - API 33+: the platform `LocaleManager.applicationLocales` (persisted by the system, listed in `res/xml/locales_config.xml`
 *   through `android:localeConfig`); changing it recreates the activity. A change made in the system per-app language screen is
 *   copied back into [SettingsRepository] on start ([adoptSystemChoice]) so both stay in sync.
 * - API 26..32: [wrap] in `MainActivity.attachBaseContext` builds a configuration context with the stored language (read from the
 *   Koin [SettingsRepository], started in `TatakaiApplication.onCreate` before any activity), and [apply] recreates the activity.
 */
object AppLocale {
    /** Language the current activity context was built with on API < 33 (null = system). */
    @Volatile private var wrappedTag: String? = null

    fun wrap(base: Context): Context {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) return base
        val tag = storedLanguage()?.tag
        wrappedTag = tag
        if (tag == null) return base
        val locale = Locale.forLanguageTag(tag)
        Locale.setDefault(locale)
        val config = Configuration(base.resources.configuration)
        config.setLocales(LocaleList(locale))
        return base.createConfigurationContext(config)
    }

    /** Applies [language] to the running app; no-op when it is already active. */
    fun apply(activity: Activity, language: AppLanguage) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val lm = activity.getSystemService(LocaleManager::class.java) ?: return
            val target = language.tag?.let { LocaleList.forLanguageTags(it) } ?: LocaleList.getEmptyLocaleList()
            if (lm.applicationLocales.toLanguageTags() != target.toLanguageTags()) lm.applicationLocales = target
        } else if (language.tag != wrappedTag) {
            activity.recreate()
        }
    }

    /** API 33+: the language chosen in the system per-app language screen, mapped to [AppLanguage]; null below 33. */
    fun systemChoice(context: Context): AppLanguage? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return null
        val lm = context.getSystemService(LocaleManager::class.java) ?: return null
        val locales = lm.applicationLocales
        if (locales.isEmpty) return AppLanguage.SYSTEM
        return when (locales[0].language) {
            "es" -> AppLanguage.ES
            "en" -> AppLanguage.EN
            else -> AppLanguage.SYSTEM
        }
    }

    private fun storedLanguage(): AppLanguage? =
        runCatching { GlobalContext.getOrNull()?.getOrNull<SettingsRepository>()?.settings?.value?.language }.getOrNull()
}
