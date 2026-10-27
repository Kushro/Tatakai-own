package app.tatakai.next.model

/** Thrown by repositories (MetadataRepository etc.) so ViewModels can render [error] directly. */
class AppException(val error: AppError, cause: Throwable? = null) : Exception(error.toString(), cause)

/** Maps any throwable to an [AppError]; [AppException] carries its own. */
fun Throwable.toAppError(): AppError = when (this) {
    is AppException -> error
    else -> AppError.Unknown(message ?: this::class.simpleName.orEmpty())
}

/** Sign-in succeeded but `profiles.is_banned = true`: the app shows "account suspended" and does not sync. */
class AccountSuspendedException : Exception("Account suspended")
