use crate::config::Settings;
use crate::interop::rspamd::{rspamd_learn_ham, rspamd_learn_spam};
use anyhow::Result;
use mastodon_async::entities::admin::Report;

/// Examine one report from a webhook event.
/// If it's a closed spam report and learning is turned on,
/// train the spam filter based on the results of the report.
pub async fn handle_report(settings: &Settings, domain: &str, report: &Report) -> Result<()> {
    if !report.action_taken {
        return Ok(());
    }
    if !report.category.is_spam() {
        return Ok(());
    }
    let Some(rspamd) = settings.rspamd.as_ref() else {
        return Ok(());
    };

    // Assume the current status of the target account is due to this report.
    // TODO: if a silenced spam account has a non-spam post reported and that report is closed as not spam, the post will still be trained as spam. Can we fix this?
    if report.target_account.silenced
        || report.target_account.suspended
        || report.target_account.disabled
    {
        for status in &report.statuses {
            rspamd_learn_spam(rspamd, domain, status).await?;
        }
    } else {
        for status in &report.statuses {
            rspamd_learn_ham(rspamd, domain, status).await?;
        }
    }

    Ok(())
}
