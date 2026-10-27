package app.tatakai.next.feature.search

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.viewmodel.koinViewModel

@Composable
fun SearchScreen(
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: SearchViewModel = koinViewModel(),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    SearchContent(
        state = state,
        onQueryChange = viewModel::onQueryChange,
        onClear = viewModel::clear,
        onRetry = viewModel::retry,
        onLoadMore = viewModel::loadMore,
        onMediaClick = onMediaClick,
        modifier = modifier,
    )
}

@Composable
internal fun SearchContent(
    state: SearchUiState,
    onQueryChange: (String) -> Unit,
    onClear: () -> Unit,
    onRetry: () -> Unit,
    onLoadMore: () -> Unit,
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    val focusManager = LocalFocusManager.current
    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = {
            TextField(
                value = state.query,
                onValueChange = onQueryChange,
                singleLine = true,
                placeholder = { Text(stringResource(R.string.search_hint)) },
                leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                trailingIcon = {
                    if (state.query.isNotEmpty()) {
                        IconButton(onClick = onClear) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.search_clear))
                        }
                    }
                },
                shape = CircleShape,
                colors = TextFieldDefaults.colors(
                    focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                ),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { focusManager.clearFocus() }),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (val results = state.results) {
                SearchResults.Idle -> EmptyState(
                    icon = Icons.Outlined.Search,
                    title = stringResource(R.string.search_idle_title),
                    message = stringResource(R.string.search_idle_message),
                    modifier = Modifier.align(Alignment.Center),
                )
                SearchResults.Loading -> LoadingState()
                SearchResults.Empty -> EmptyState(
                    icon = Icons.Outlined.SearchOff,
                    title = stringResource(R.string.search_empty_title, state.query.trim()),
                    message = stringResource(R.string.search_empty_message),
                    modifier = Modifier.align(Alignment.Center),
                )
                is SearchResults.Err -> ErrorState(
                    message = results.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = onRetry,
                    modifier = Modifier.align(Alignment.Center),
                    title = stringResource(UiR.string.ui_error_title),
                )
                is SearchResults.Ok -> ResultsGrid(results, onMediaClick, onLoadMore)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ResultsGrid(results: SearchResults.Ok, onMediaClick: (Media) -> Unit, onLoadMore: () -> Unit) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 110.dp),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(results.items, key = { mediaKey(it) }, contentType = { "media" }) { media ->
            MediaCard(media = media, onClick = { onMediaClick(media) })
        }
        if (results.hasNext) {
            item(key = "load-more", span = { GridItemSpan(maxLineSpan) }, contentType = "load-more") {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    if (results.loadingMore) {
                        LoadingIndicator(Modifier.padding(8.dp).size(40.dp))
                    } else {
                        OutlinedButton(onClick = onLoadMore) { Text(stringResource(R.string.search_load_more)) }
                    }
                }
            }
        }
    }
}
