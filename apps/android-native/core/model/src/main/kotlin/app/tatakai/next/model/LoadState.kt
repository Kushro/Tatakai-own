package app.tatakai.next.model

sealed interface LoadState<out T> {
    data object Loading : LoadState<Nothing>
    data class Ok<T>(val value: T) : LoadState<T>
    data class Err(val cause: AppError) : LoadState<Nothing>
}

sealed interface AppError {
    data object Offline : AppError
    data class RateLimited(val provider: String) : AppError
    data class Extension(val id: String, val msg: String) : AppError
    data class Http(val code: Int) : AppError
    data class Unknown(val msg: String) : AppError
}
