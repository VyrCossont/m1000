use crate::config::{Report, Restrict, Settings};
use crate::interop::rspamd::rspamd_scan;
use crate::pattern::{Matcher, RuleMatcherInput};
use crate::CompiledConfig;
use mastodon_async::admin::{AccountAction, AccountActionRequest};
use mastodon_async::entities::report::Category;
use mastodon_async::entities::{AccountId, ReportId, RuleId};
use mastodon_async::prelude::Status;
use mastodon_async::{AddReportRequest, Mastodon};
use std::collections::HashSet;
use tracing::{error, info};

/// Examine one status from a webhook event to see if it matches any rules.
/// If so, report the status and/or restrict the account.
pub async fn handle_status(
    settings: &Settings,
    config: &CompiledConfig,
    mastodon: &Mastodon,
    status: &Status,
) -> anyhow::Result<()> {
    let mut report_builder: Option<ReportBuilder> = None;
    let mut highest_restrict: Option<Restrict> = None;
    let mut rule_matcher_input = RuleMatcherInput::from(status);

    if let Some(rspamd) = settings.rspamd.as_ref() {
        let action = rspamd_scan(rspamd, &config.domain, status).await?;
        rule_matcher_input.rspamd(action);
    }

    for rule in config.rules.iter() {
        if rule
            .matchers
            .iter()
            .any(|matcher| matcher.is_match(&rule_matcher_input))
        {
            if let Some(report) = rule.report.as_ref() {
                report_builder
                    .get_or_insert_with(|| Default::default())
                    .rule_violation(&rule.name, report);
            }

            if let Some(restrict) = rule.restrict {
                if let Some(existing_restrict) = highest_restrict {
                    if restrict > existing_restrict {
                        highest_restrict = Some(restrict);
                    }
                } else {
                    highest_restrict = Some(restrict);
                }
            }
        }
    }

    let report_id = if let Some(report_builder) = report_builder {
        let result = report_status(config, mastodon, status, report_builder).await;
        if let Some(e) = result.as_ref().err() {
            error!(
                "Couldn't create report for status {status_id}: {e}",
                status_id = status.id
            );
        }
        result.ok()
    } else {
        None
    };

    if let Some(restrict) = highest_restrict {
        restrict_account(mastodon, &status.account.id, restrict, report_id).await?;
    }

    Ok(())
}

/// Report an account and status.
/// Optionally forward that report to the origin server.
async fn report_status(
    config: &CompiledConfig,
    mastodon: &Mastodon,
    status: &Status,
    report_builder: ReportBuilder,
) -> anyhow::Result<ReportId> {
    let mut api_report_builder = AddReportRequest::builder(status.account.id.clone());
    api_report_builder.status_ids(vec![status.id.clone()]);

    let mut rule_names_list = report_builder
        .rule_names
        .into_iter()
        .map(|name| format!("- {name}"))
        .collect::<Vec<_>>();
    rule_names_list.sort();
    api_report_builder.comment(format!(
        "Automod rules broken:\n{}",
        rule_names_list.join("\n")
    ));

    if !report_builder.rule_ids.is_empty() {
        // Violation of specific instance rules with IDs.
        api_report_builder.category(Category::Violation);
        api_report_builder.rule_ids(report_builder.rule_ids.into_iter().collect::<Vec<_>>());
    } else if report_builder.spam {
        // Spam. Lower priority than specific rule violations.
        api_report_builder.category(Category::Spam);
    } else {
        // Not related to instance rules or spam.
        api_report_builder.category(Category::Other);
    }

    api_report_builder.forward(report_builder.forward);

    let report = mastodon.add_report(&api_report_builder.build()).await?;
    let username = &config.username;
    let domain = &config.domain;
    info!("{username}@{domain}: Filed report: {:#?}", report);

    Ok(report.id)
}

/// Restrict an account: silence, suspend, etc.
/// Can take a report ID from a previous report for audit trail purposes.
async fn restrict_account(
    mastodon: &Mastodon,
    account_id: &AccountId,
    restrict: Restrict,
    report_id: Option<ReportId>,
) -> anyhow::Result<()> {
    let mut action_builder = AccountActionRequest::builder(match restrict {
        Restrict::Sensitive => AccountAction::Sensitive,
        Restrict::Disable => AccountAction::Disable,
        Restrict::Silence => AccountAction::Silence,
        Restrict::Suspend => AccountAction::Suspend,
    });
    if let Some(report_id) = report_id {
        action_builder.report_id(report_id);
    }
    let action_request = action_builder.build();

    mastodon
        .admin_perform_account_action(account_id, &action_request)
        .await?;

    Ok(())
}

/// Accumulate the text and machine-readable info for a report.
/// Not to be confused with the Mastodon API request builder for a report.
#[derive(Debug, Default)]
struct ReportBuilder {
    /// Names from our config file, not the server's rules.
    rule_names: HashSet<String>,
    /// These IDs are for the server's rules.
    rule_ids: HashSet<RuleId>,
    /// Is this considered spam? Will be ignored if any rule IDs are set.
    spam: bool,
    /// Should we forward this report to the user's home server?
    forward: bool,
}

impl ReportBuilder {
    fn rule_violation(&mut self, rule_name: &String, report: &Report) -> &mut Self {
        self.rule_names.insert(rule_name.clone());
        self.rule_ids
            .extend(report.rule_ids.iter().map(RuleId::new));
        self.spam |= report.spam;
        self.forward |= report.forward;
        self
    }
}
