package app.tatakai.next.ui.state

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.ui.R
import java.io.IOException
import kotlin.coroutines.cancellation.CancellationException

/**
 * Maps a failure of a repository call to an [AppError]. Never swallows cancellation: callers must rethrow it first
 * (`catch (e: CancellationException) { throw e }`) or use [runCatchingApp].
 */
fun Throwable.toAppError(): AppError = when (this) {
    is AppException -> error
    is IOException -> AppError.Offline
    else -> AppError.Unknown(message?.takeIf { it.isNotBlank() } ?: this::class.java.simpleName)
}

/** Like [runCatching] but rethrows [CancellationException] and returns the failure already mapped to [AppError]. */
suspend inline fun <T> runCatchingApp(block: () -> T): Result<T> = try {
    Result.success(block())
} catch (e: CancellationException) {
    throw e
} catch (e: Exception) {
    Result.failure(e)
}

/** Human readable, localized cause (02 §5: "AniList limited requests", "extension X failed: timeout"). */
@Composable
fun AppError.userMessage(): String = when (this) {
    AppError.Offline -> stringResource(R.string.ui_error_offline)
    is AppError.RateLimited -> stringResource(R.string.ui_error_rate_limited, provider)
    is AppError.Extension -> stringResource(R.string.ui_error_extension, id, msg)
    is AppError.Http -> stringResource(R.string.ui_error_http, code)
    is AppError.Unknown -> stringResource(R.string.ui_error_unknown, msg)
}
