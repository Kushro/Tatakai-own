package app.tatakai.next.feature.settings

import android.os.Build
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.SingleChoiceButtonGroup
import app.tatakai.next.designsystem.generated.PaletteInfo
import app.tatakai.next.designsystem.generated.TatakaiTokens
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.ui.state.screenInsets
import org.koin.compose.viewmodel.koinViewModel

@Composable
fun SettingsScreen(
    onOpenExtensions: () -> Unit,
    onOpenLink: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: SettingsViewModel = koinViewModel(),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    SettingsContent(
        state = state,
        onPalette = viewModel::setPalette,
        onAmoled = viewModel::setAmoled,
        onDynamic = viewModel::setDynamicColor,
        onContrast = viewModel::setContrast,
        onMoveProvider = viewModel::moveProvider,
        onSignIn = viewModel::signIn,
        onSignOut = viewModel::signOut,
        onOpenExtensions = onOpenExtensions,
        onOpenLink = onOpenLink,
        modifier = modifier,
    )
}

@Composable
internal fun SettingsContent(
    state: SettingsUiState,
    onPalette: (String) -> Unit,
    onAmoled: (Boolean) -> Unit,
    onDynamic: (Boolean) -> Unit,
    onContrast: (ContrastPref) -> Unit,
    onMoveProvider: (String, Int) -> Unit,
    onSignIn: (String, String) -> Unit,
    onSignOut: () -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLink: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Scaffold(modifier = modifier, contentWindowInsets = screenInsets) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item(key = "title", contentType = "title") {
                Text(stringResource(R.string.settings_title), style = MaterialTheme.typography.headlineMedium)
            }
            item(key = "appearance", contentType = "section") {
                AppearanceSection(state, onPalette, onAmoled, onDynamic, onContrast)
            }
            item(key = "metadata", contentType = "section") {
                MetadataSection(state.settings.providerPriority, onMoveProvider)
            }
            item(key = "account", contentType = "section") {
                AccountSection(state, onSignIn, onSignOut)
            }
            item(key = "more", contentType = "section") {
                SectionCard(title = stringResource(R.string.settings_more)) {
                    NavRow(Icons.Outlined.Extension, stringResource(R.string.settings_extensions), onOpenExtensions)
                    NavRow(Icons.Outlined.Computer, stringResource(R.string.settings_link), onOpenLink)
                }
            }
        }
    }
}

@Composable
private fun AppearanceSection(
    state: SettingsUiState,
    onPalette: (String) -> Unit,
    onAmoled: (Boolean) -> Unit,
    onDynamic: (Boolean) -> Unit,
    onContrast: (ContrastPref) -> Unit,
) {
    val s = state.settings
    SectionCard(title = stringResource(R.string.settings_appearance)) {
        Text(stringResource(R.string.settings_palette), style = MaterialTheme.typography.labelLarge)
        LazyRow(
            contentPadding = PaddingValues(vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(TatakaiTokens.paletteInfo, key = { it.id }, contentType = { "palette" }) { info ->
                PaletteSwatch(info, selected = info.id == s.paletteId && !(s.dynamicColor && dynamicSupported), onClick = { onPalette(info.id) })
            }
        }
        SwitchRow(stringResource(R.string.settings_amoled), s.amoled, onAmoled)
        if (dynamicSupported) {
            SwitchRow(stringResource(R.string.settings_dynamic), s.dynamicColor, onDynamic)
        }
        Text(
            stringResource(R.string.settings_contrast),
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(top = 8.dp, bottom = 8.dp),
        )
        SingleChoiceButtonGroup(
            options = ContrastPref.entries,
            selected = s.contrast,
            label = { stringResource(it.labelRes()) },
            onSelected = onContrast,
        )
    }
}

private val dynamicSupported: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

private fun ContrastPref.labelRes(): Int = when (this) {
    ContrastPref.STANDARD -> R.string.settings_contrast_standard
    ContrastPref.MEDIUM -> R.string.settings_contrast_medium
    ContrastPref.HIGH -> R.string.settings_contrast_high
}

@Composable
private fun PaletteSwatch(info: PaletteInfo, selected: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier
                .size(48.dp)
                .clip(CircleShape)
                .background(info.seed)
                .then(if (selected) Modifier.border(BorderStroke(3.dp, MaterialTheme.colorScheme.primary), CircleShape) else Modifier)
                .clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) Icon(Icons.Filled.Check, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimary)
        }
        Text(info.label, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(top = 4.dp))
    }
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!checked) }.padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
private fun MetadataSection(priority: List<String>, onMove: (String, Int) -> Unit) {
    SectionCard(title = stringResource(R.string.settings_metadata)) {
        Text(
            stringResource(R.string.settings_metadata_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 8.dp),
        )
        priority.forEachIndexed { index, id ->
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("${index + 1}. ${providerName(id)}", Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                IconButton(onClick = { onMove(id, -1) }, enabled = index > 0) {
                    Icon(Icons.Filled.KeyboardArrowUp, contentDescription = stringResource(R.string.settings_move_up))
                }
                IconButton(onClick = { onMove(id, 1) }, enabled = index < priority.lastIndex) {
                    Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.settings_move_down))
                }
            }
        }
    }
}

/** Proper names of the metadata providers (not translated). */
private fun providerName(id: String): String = when (id) {
    "anilist" -> "AniList"
    "anizip" -> "ani.zip"
    "kitsu" -> "Kitsu"
    else -> id
}

@Composable
private fun AccountSection(state: SettingsUiState, onSignIn: (String, String) -> Unit, onSignOut: () -> Unit) {
    SectionCard(title = stringResource(R.string.settings_account)) {
        val session = state.session
        if (session != null) {
            Text(
                stringResource(R.string.settings_signed_in_as, session.email ?: session.userId),
                style = MaterialTheme.typography.bodyLarge,
            )
            OutlinedButton(onClick = onSignOut, modifier = Modifier.padding(top = 8.dp)) {
                Text(stringResource(R.string.settings_sign_out))
            }
        } else {
            var email by rememberSaveable { mutableStateOf("") }
            var password by rememberSaveable { mutableStateOf("") }
            Text(
                stringResource(R.string.settings_account_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = email,
                onValueChange = { email = it },
                label = { Text(stringResource(R.string.settings_email)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                label = { Text(stringResource(R.string.settings_password)) },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
            state.signInError?.let {
                Text(
                    stringResource(if (it == SignInError.SUSPENDED) R.string.settings_error_suspended else R.string.settings_error_invalid),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
            Button(
                onClick = { onSignIn(email, password) },
                enabled = !state.signingIn && email.isNotBlank() && password.isNotEmpty(),
                modifier = Modifier.padding(top = 12.dp),
            ) {
                Text(stringResource(if (state.signingIn) R.string.settings_signing_in else R.string.settings_sign_in))
            }
        }
    }
}

@Composable
private fun NavRow(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(label) },
        leadingContent = { Icon(icon, contentDescription = null) },
        trailingContent = { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null) },
        modifier = Modifier.clip(MaterialTheme.shapes.medium).clickable(onClick = onClick),
    )
}
