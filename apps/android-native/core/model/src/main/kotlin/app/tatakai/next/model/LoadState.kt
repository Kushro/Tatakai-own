package app.tatakai.next.model

sealed interface LoadState<out T> {
    data object Loading : LoadState<Nothing>
    data class Ok<T>(val value: T) : LoadState<T>
    data class Err(val cause: AppError) : LoadState<Nothing>
}

sealed interface AppError {
    data object Offline : AppError
    data class RateLimited(val provider: String) : AppError {
        /**
         * 0.3.1: how long the provider asked to wait (`Retry-After`, or the AniList gateway's own budget wait), null when unknown.
         * Informational only: it is not part of equality.
         */
        var retryAfterMs: Long? = null
            private set

        constructor(provider: String, retryAfterMs: Long?) : this(provider) {
            this.retryAfterMs = retryAfterMs
        }
    }
    data class Extension(val id: String, val msg: String) : AppError
    data class Http(val code: Int) : AppError
    data class Unknown(val msg: String) : AppError
}
