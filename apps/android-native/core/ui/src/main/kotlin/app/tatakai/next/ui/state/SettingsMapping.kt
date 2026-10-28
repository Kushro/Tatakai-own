package app.tatakai.next.ui.state

import app.tatakai.next.designsystem.generated.ContrastLevel
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContrastPref

fun AppSettings.contrastLevel(): ContrastLevel = contrast.toLevel()

fun ContrastPref.toLevel(): ContrastLevel = when (this) {
    ContrastPref.STANDARD -> ContrastLevel.Standard
    ContrastPref.MEDIUM -> ContrastLevel.Medium
    ContrastPref.HIGH -> ContrastLevel.High
}
