package app.tatakai.next.ui.state

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.model.LoadState
import app.tatakai.next.ui.R

/** Renders the three states of a [LoadState] with the shared Loading / Error components. */
@Composable
fun <T> LoadStateContent(
    state: LoadState<T>,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    loading: @Composable () -> Unit = { LoadingState(modifier) },
    content: @Composable (T) -> Unit,
) {
    when (state) {
        LoadState.Loading -> loading()
        is LoadState.Err -> ErrorState(
            message = state.cause.userMessage(),
            retryLabel = stringResource(R.string.ui_retry),
            onRetry = onRetry,
            modifier = modifier,
            title = stringResource(R.string.ui_error_title),
        )
        is LoadState.Ok -> content(state.value)
    }
}
