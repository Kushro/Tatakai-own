package app.tatakai.next.feature.search

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.time.Year

private val SEASONS = listOf("WINTER", "SPRING", "SUMMER", "FALL")
private val STATUSES = listOf("RELEASING", "FINISHED", "NOT_YET_RELEASED")
private const val YEARS = 10

/** Filters bottom sheet (prototype `openFilters`): genre, year, season, status as toggle chips; Reset / Apply. Draft until Apply. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun SearchFiltersSheet(
    current: SearchFilters,
    genreOptions: List<String>,
    onApply: (SearchFilters) -> Unit,
    onDismiss: () -> Unit,
) {
    var draft by remember(current) { mutableStateOf(current) }
    val years = remember { (Year.now().value downTo Year.now().value - YEARS + 1).toList() }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .padding(horizontal = 24.dp)
                .padding(bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(stringResource(R.string.search_filters), style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
            FilterGroup(stringResource(R.string.search_genre)) {
                genreOptions.forEach { g ->
                    FilterChip(
                        selected = draft.genre == g,
                        onClick = { draft = draft.copy(genre = if (draft.genre == g) null else g) },
                        label = { Text(g) },
                    )
                }
            }
            FilterGroup(stringResource(R.string.search_year)) {
                years.forEach { y ->
                    FilterChip(
                        selected = draft.year == y,
                        onClick = { draft = draft.copy(year = if (draft.year == y) null else y) },
                        label = { Text(y.toString()) },
                    )
                }
            }
            FilterGroup(stringResource(R.string.search_season)) {
                SEASONS.forEach { s ->
                    FilterChip(
                        selected = draft.season == s,
                        onClick = { draft = draft.copy(season = if (draft.season == s) null else s) },
                        label = { Text(seasonLabel(s)) },
                    )
                }
            }
            FilterGroup(stringResource(R.string.search_status)) {
                STATUSES.forEach { s ->
                    FilterChip(
                        selected = draft.status == s,
                        onClick = { draft = draft.copy(status = if (draft.status == s) null else s) },
                        label = { Text(statusLabel(s) ?: s) },
                    )
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, androidx.compose.ui.Alignment.End)) {
                TextButton(onClick = { onApply(SearchFilters()) }) { Text(stringResource(R.string.search_reset)) }
                Button(onClick = { onApply(draft) }) { Text(stringResource(R.string.search_apply)) }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FilterGroup(title: String, chips: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            chips()
        }
    }
}
