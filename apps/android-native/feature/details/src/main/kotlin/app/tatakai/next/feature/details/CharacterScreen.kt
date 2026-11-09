// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalLayoutApi::class)

package app.tatakai.next.feature.details

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.model.Character
import app.tatakai.next.model.Media
import app.tatakai.next.model.StaffRef
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.LoadStateContent
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf

/**
 * Classic `/character/:name` page: image, name + native name, facts (age, birthday, blood type, gender, favourites), description with
 * More / Less, voice actors, appearances grid ([onMediaClick] -> Details), favorite heart (ListsRepository of worker q). Data:
 * `MetadataRepository.character(id)`. Rendered in the detail pane on expanded windows (same `ListDetailSceneStrategy.detailPane()` as
 * Details). Loading / error (404 included) through LoadStateContent; empty sections are hidden.
 */
@Composable
fun CharacterScreen(
    characterId: Int,
    onBack: () -> Unit,
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel: CharacterViewModel = koinViewModel(key = "character-$characterId", parameters = { parametersOf(characterId) })
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is CharacterEvent.FavoriteFailed ->
                    snackbar.show(resources.getString(R.string.details_character_favorite_failed, event.cause.describe(resources)))
            }
        }
    }
    Box(modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        LoadStateContent(state = state.character, onRetry = viewModel::retry) { character ->
            CharacterBody(
                character = character,
                favorite = state.favorite,
                saving = state.savingFavorite,
                onToggleFavorite = viewModel::toggleFavorite,
                onMediaClick = onMediaClick,
            )
        }
        IconButton(onClick = onBack, modifier = Modifier.statusBarsPadding().padding(4.dp)) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
        }
    }
}

@Composable
private fun CharacterBody(
    character: Character,
    favorite: Boolean?,
    saving: Boolean,
    onToggleFavorite: () -> Unit,
    onMediaClick: (Media) -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .statusBarsPadding()
            .navigationBarsPadding()
            .padding(top = 48.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(
            Modifier.padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            MediaCover(url = character.image, modifier = Modifier.width(120.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(character.name, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                character.nativeName?.takeIf { it != character.name }?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                character.favourites?.let {
                    Text(
                        stringResource(R.string.details_character_favourites, it),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (favorite != null) {
                IconToggleButton(checked = favorite, onCheckedChange = { onToggleFavorite() }, enabled = !saving) {
                    Icon(
                        if (favorite) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder,
                        contentDescription = stringResource(
                            if (favorite) R.string.details_character_unfavorite else R.string.details_character_favorite,
                        ),
                        tint = if (favorite) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        val facts = listOfNotNull(
            character.age?.let { stringResource(R.string.details_character_age, it) },
            character.birthday?.let { stringResource(R.string.details_character_birthday, it) },
            character.bloodType?.let { stringResource(R.string.details_character_blood, it) },
            character.gender?.let { stringResource(R.string.details_character_gender, it) },
        )
        if (facts.isNotEmpty()) {
            FlowRow(
                Modifier.padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) { facts.forEach { InfoChip(it) } }
        }
        character.description?.takeIf { it.isNotBlank() }?.let { description ->
            Column(Modifier.padding(horizontal = 16.dp)) {
                Text(
                    description,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = if (expanded) Int.MAX_VALUE else DESCRIPTION_LINES,
                    overflow = TextOverflow.Ellipsis,
                )
                if (description.length > DESCRIPTION_TOGGLE_CHARS) {
                    TextButton(onClick = { expanded = !expanded }) {
                        Text(stringResource(if (expanded) R.string.details_less else R.string.details_more))
                    }
                }
            }
        }
        if (character.voiceActors.isNotEmpty()) {
            SectionCard(modifier = Modifier.padding(horizontal = 16.dp), title = stringResource(R.string.details_character_voice_actors)) {
                character.voiceActors.forEach { va -> VoiceActorRow(va) }
            }
        }
        if (character.appearances.isNotEmpty()) {
            Text(
                stringResource(R.string.details_character_appearances),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                character.appearances.forEach { appearance ->
                    Column(Modifier.width(120.dp)) {
                        MediaCard(media = appearance.media, onClick = { onMediaClick(appearance.media) })
                        appearance.role?.let {
                            Text(
                                characterRoleLabel(it),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun VoiceActorRow(va: StaffRef) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = va.image, modifier = Modifier.width(40.dp), shape = CircleShape, aspectRatio = 1f)
        Column(Modifier.weight(1f)) {
            Text(va.name, style = MaterialTheme.typography.bodyLarge)
            va.language?.let {
                Text(languageLabel(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** AniList `CharacterRole`: MAIN / SUPPORTING / BACKGROUND. */
@Composable
internal fun characterRoleLabel(role: String): String = when (role.uppercase()) {
    "MAIN" -> stringResource(R.string.details_role_main)
    "SUPPORTING" -> stringResource(R.string.details_role_supporting)
    "BACKGROUND" -> stringResource(R.string.details_role_background)
    else -> role
}

/** AniList `StaffLanguage` words (`JAPANESE`, `ENGLISH`...) shown capitalised: they are language names, not app text. */
internal fun languageLabel(language: String): String =
    language.lowercase().replaceFirstChar { it.titlecase() }

private const val DESCRIPTION_LINES = 6
private const val DESCRIPTION_TOGGLE_CHARS = 280
