//! Deletes what is kept long enough. The periods are the hosted service's
//! privacy policy, and a self-hosted instance keeps no longer.

use sqlx::PgPool;

/// Deletes site assistant questions, raw support mail and feedback
/// screenshots after 90 days, and support conversations and feedback reports
/// two years after their last message or change. A message's text stays
/// with its conversation when its raw mail is deleted.
///
/// # Errors
///
/// A database error.
pub async fn forget(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM assistant_questions WHERE created_at < now() - interval '90 days'")
        .execute(db)
        .await?;
    sqlx::query!("DELETE FROM inbound_mail WHERE received_at < now() - interval '90 days'")
        .execute(db)
        .await?;
    sqlx::query!(
        "DELETE FROM support_conversations c
         WHERE c.created_at < now() - interval '2 years'
           AND NOT EXISTS (
             SELECT 1 FROM support_messages m
             WHERE m.conversation_id = c.id AND m.created_at >= now() - interval '2 years'
           )"
    )
    .execute(db)
    .await?;
    sqlx::query!(
        "UPDATE feedback_reports SET screenshot = NULL, screenshot_type = NULL
         WHERE screenshot IS NOT NULL AND created_at < now() - interval '90 days'"
    )
    .execute(db)
    .await?;
    sqlx::query!("DELETE FROM feedback_reports WHERE updated_at < now() - interval '2 years'")
        .execute(db)
        .await?;
    Ok(())
}
