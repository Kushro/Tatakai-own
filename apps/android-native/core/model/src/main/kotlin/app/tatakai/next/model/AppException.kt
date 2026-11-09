package app.tatakai.next.model

/** Thrown by repositories (MetadataRepository etc.) so ViewModels can render [error] directly. */
class AppException(val error: AppError, cause: Throwable? = null) : Exception(error.toString(), cause)

/** Maps any throwable to an [AppError]; [AppException] carries its own. */
fun Throwable.toAppError(): AppError = when (this) {
    is AppException -> error
    else -> AppError.Unknown(message ?: this::class.simpleName.orEmpty())
}

// 2026-11-09 amendment (plan 14 §0.1 #9): AccountSuspendedException / EmailNotConfirmedException (hosted-backend sign-in) were removed with
// the own login. OAuth failures of the list providers travel as AppException(AppError.Http(status)).
