// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import android.content.Context

// Parity wave B (16 §3.7, worker t): the sentence of each AniList notification type, localised (en + es resources of :core:social).
// AniList sends English `contexts`; the app builds its own sentence from the type and the user / title / episode, and keeps AniList's
// text only for a type it does not know.

/** Builds `SocialNotification.text`. [fallback] = AniList's own sentence (see `SocialParsers.contextText`). */
fun interface SocialTexts {
    fun notification(type: String, userName: String?, mediaTitle: String?, episode: Int?, fallback: String): String
}

/** The template keys per AniList `NotificationType` (the string resource is `social_notification_<key>`). */
object SocialNotificationTypes {
    /** type -> (template key, what fills `%1$s` / `%2$s`). */
    val TEMPLATES: Map<String, String> = mapOf(
        "AIRING" to "airing",
        "FOLLOWING" to "following",
        "ACTIVITY_MESSAGE" to "activity_message",
        "ACTIVITY_MENTION" to "activity_mention",
        "ACTIVITY_REPLY" to "activity_reply",
        "ACTIVITY_REPLY_SUBSCRIBED" to "activity_reply_subscribed",
        "ACTIVITY_LIKE" to "activity_like",
        "ACTIVITY_REPLY_LIKE" to "activity_reply_like",
        "THREAD_COMMENT_MENTION" to "thread_comment_mention",
        "THREAD_COMMENT_REPLY" to "thread_comment_reply",
        "THREAD_SUBSCRIBED" to "thread_subscribed",
        "THREAD_LIKE" to "thread_like",
        "THREAD_COMMENT_LIKE" to "thread_comment_like",
        "RELATED_MEDIA_ADDITION" to "related_media_addition",
        "MEDIA_DATA_CHANGE" to "media_data_change",
        "MEDIA_MERGE" to "media_merge",
        "MEDIA_DELETION" to "media_deletion",
    )

    /** Types whose sentence is about a title (`%1$s` = title) instead of a user. */
    val MEDIA_TYPES: Set<String> = setOf("RELATED_MEDIA_ADDITION", "MEDIA_DATA_CHANGE", "MEDIA_MERGE", "MEDIA_DELETION")
}

/**
 * Pure part of the localisation: picks the template of [type] from [templates] (key -> pattern with `%1$s` / `%2$s`) and fills it;
 * an unknown type or a missing argument returns [fallback]. Shared by the Android implementation and the unit tests.
 */
fun localizedNotification(
    templates: (String) -> String?,
    type: String,
    userName: String?,
    mediaTitle: String?,
    episode: Int?,
    fallback: String,
): String {
    val key = SocialNotificationTypes.TEMPLATES[type] ?: return fallback
    val pattern = templates(key) ?: return fallback
    return when {
        type == "AIRING" -> if (episode != null && mediaTitle != null) pattern.format(episode.toString(), mediaTitle) else fallback
        type in SocialNotificationTypes.MEDIA_TYPES -> if (mediaTitle != null) pattern.format(mediaTitle) else fallback
        else -> if (userName != null) pattern.format(userName) else fallback
    }
}

/** English templates (used by tests and as the code default when no Context is given). */
val ENGLISH_TEMPLATES: Map<String, String> = mapOf(
    "airing" to "Episode %1\$s of %2\$s aired.",
    "following" to "%1\$s started following you.",
    "activity_message" to "%1\$s sent you a message.",
    "activity_mention" to "%1\$s mentioned you in an activity.",
    "activity_reply" to "%1\$s replied to your activity.",
    "activity_reply_subscribed" to "%1\$s replied to an activity you follow.",
    "activity_like" to "%1\$s liked your activity.",
    "activity_reply_like" to "%1\$s liked your reply.",
    "thread_comment_mention" to "%1\$s mentioned you in a forum comment.",
    "thread_comment_reply" to "%1\$s replied to your forum comment.",
    "thread_subscribed" to "%1\$s commented in a thread you follow.",
    "thread_like" to "%1\$s liked your forum thread.",
    "thread_comment_like" to "%1\$s liked your forum comment.",
    "related_media_addition" to "%1\$s was added to AniList.",
    "media_data_change" to "%1\$s was updated on AniList.",
    "media_merge" to "%1\$s was merged with another entry.",
    "media_deletion" to "%1\$s was removed from AniList.",
)

object EnglishSocialTexts : SocialTexts {
    override fun notification(type: String, userName: String?, mediaTitle: String?, episode: Int?, fallback: String): String =
        localizedNotification(ENGLISH_TEMPLATES::get, type, userName, mediaTitle, episode, fallback)
}

/** Templates from the app's resources (`values` / `values-es`), so the sentence follows the app language. */
class AndroidSocialTexts(private val context: Context) : SocialTexts {
    override fun notification(type: String, userName: String?, mediaTitle: String?, episode: Int?, fallback: String): String =
        localizedNotification(::template, type, userName, mediaTitle, episode, fallback)

    private fun template(key: String): String? {
        val id = RES[key] ?: return null
        return runCatching { context.getString(id) }.getOrNull()
    }

    private companion object {
        val RES: Map<String, Int> = mapOf(
            "airing" to R.string.social_notification_airing,
            "following" to R.string.social_notification_following,
            "activity_message" to R.string.social_notification_activity_message,
            "activity_mention" to R.string.social_notification_activity_mention,
            "activity_reply" to R.string.social_notification_activity_reply,
            "activity_reply_subscribed" to R.string.social_notification_activity_reply_subscribed,
            "activity_like" to R.string.social_notification_activity_like,
            "activity_reply_like" to R.string.social_notification_activity_reply_like,
            "thread_comment_mention" to R.string.social_notification_thread_comment_mention,
            "thread_comment_reply" to R.string.social_notification_thread_comment_reply,
            "thread_subscribed" to R.string.social_notification_thread_subscribed,
            "thread_like" to R.string.social_notification_thread_like,
            "thread_comment_like" to R.string.social_notification_thread_comment_like,
            "related_media_addition" to R.string.social_notification_related_media_addition,
            "media_data_change" to R.string.social_notification_media_data_change,
            "media_merge" to R.string.social_notification_media_merge,
            "media_deletion" to R.string.social_notification_media_deletion,
        )
    }
}
