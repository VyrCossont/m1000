use crate::config::Settings;
use crate::setup::{ensure_mastodon, ensure_registered};
use anyhow::{bail, Result};
use mail_builder::headers::address::{Address, EmailAddress};
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::text::Text;
use mail_builder::headers::HeaderType;
use mail_builder::MessageBuilder;
use mastodon_async::entities::status::Status;
use mastodon_async::entities::StatusId;
use mastodon_async::Visibility;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Dump a single post as a MIME message to stdout.
pub async fn dump_as_mime(
    config_dir: &Path,
    client: &Client,
    domain: &str,
    username: &str,
    id: &str,
) -> Result<()> {
    let registered = ensure_registered(config_dir, client, domain).await?;
    let mastodon = ensure_mastodon(config_dir, registered, domain, username, false).await?;
    let status = mastodon.get_status(&StatusId::new(id)).await?;

    let message_builder = status_to_mime(domain, &status);
    message_builder.write_to(io::stdout())?;

    Ok(())
}

/// Run a MIME message version of a post through rspamd, returning the action it recommends.
pub async fn rspamd_scan(settings: &Settings, domain: &str, status: &Status) -> Result<String> {
    let Some(rspamc_command) = settings.rspamc_command.as_ref() else {
        bail!("rspamc_command is missing from global settings.");
    };
    let Some((cmd, args)) = rspamc_command.split_first() else {
        bail!("rspamc_command in global settings is an empty list. It should be a non-empty list, or not present at all.");
    };

    let mut command = Command::new(cmd);
    for arg in args {
        command.arg(arg);
    }
    command.arg("--json");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    let mut process = command.spawn()?;

    let Some(mut stdin) = process.stdin.take() else {
        bail!("Couldn't get rspamc stdin");
    };
    let message_builder = status_to_mime(domain, &status);
    let message_bytes = message_builder.write_to_vec()?;
    stdin.write(message_bytes.as_slice()).await?;
    drop(stdin);

    let output = process.wait_with_output().await?;
    if !output.status.success() {
        bail!("rspamc exited with code {code}", code = output.status);
    }

    // Drop the first line, which isn't part of the JSON output.
    let Some(end_of_first_line) = output.stdout.iter().position(|b| *b == b'\n') else {
        bail!("Unexpected format of rspamc output");
    };
    if end_of_first_line + 1 >= output.stdout.len() {
        bail!("Unexpected format of rspamc output");
    }
    let json_slice = &output.stdout[end_of_first_line + 1..];

    let rspamc_result: RspamcResult = serde_json::from_slice(json_slice)?;
    Ok(rspamc_result.action)
}

/// Translate a single post to a MIME message.
fn status_to_mime<'a>(domain: &str, status: &'a Status) -> MessageBuilder<'a> {
    let mime_version_header = HeaderType::Text(Text {
        text: Cow::from("1.0"),
    });

    let message_id = MessageId {
        id: vec![Cow::from(format!("{id}@{domain}", id = &status.id))],
    };

    let from = Address::Address(EmailAddress {
        name: Some(Cow::from(&status.account.display_name)),
        email: Cow::from(if status.account.acct.contains('@') {
            status.account.acct.clone()
        } else {
            format!("{username}@{domain}", username = status.account.username)
        }),
    });

    let visibility_header = HeaderType::Text(Text {
        text: Cow::from(match status.visibility {
            Visibility::Direct => "direct",
            Visibility::Private => "private",
            Visibility::Unlisted => "unlisted",
            Visibility::Public => "public",
        }),
    });

    let sensitive_header = HeaderType::Text(Text {
        text: Cow::from(status.sensitive.to_string()),
    });

    let mut message_builder = MessageBuilder::new()
        .header("MIME-Version", mime_version_header)
        .message_id(message_id)
        .date(status.created_at.unix_timestamp())
        .from(from)
        .html_body(&status.content)
        .header("Mastodon-Visibility", visibility_header)
        .header("Mastodon-Sensitive", sensitive_header);

    if let Some(in_reply_to_id) = status.in_reply_to_id.as_ref() {
        message_builder = message_builder.in_reply_to(MessageId {
            id: vec![Cow::from(format!("{in_reply_to_id}@{domain}"))],
        })
    }

    if !status.spoiler_text.is_empty() {
        message_builder = message_builder.subject(&status.spoiler_text);
    }

    if !status.mentions.is_empty() {
        message_builder = message_builder.to(Address::List(
            status
                .mentions
                .iter()
                .map(|mention| {
                    Address::Address(EmailAddress {
                        name: None,
                        email: if mention.acct.contains('@') {
                            Cow::from(&mention.acct)
                        } else {
                            Cow::from(format!("{username}@{domain}", username = mention.username))
                        },
                    })
                })
                .collect(),
        ));
    }

    if !status.tags.is_empty() {
        message_builder = message_builder.header(
            "Keywords",
            HeaderType::Text(Text {
                text: Cow::from(
                    status
                        .tags
                        .iter()
                        .map(|tag| tag.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            }),
        );
    }

    if let Some(application) = &status.application {
        let x_mailer = if let Some(website) = &application.website {
            Cow::from(format!("{name} <{website}>", name = application.name))
        } else {
            Cow::from(&application.name)
        };
        message_builder =
            message_builder.header("X-Mailer", HeaderType::Text(Text { text: x_mailer }));
    }

    // TODO: media attachments

    message_builder
}

/// JSON output of `rspamc`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RspamcResult {
    /// Action recommended. The most common are `no action` and `reject`, but there are others:
    /// https://rspamd.com/doc/faq.html#what-are-rspamd-actions
    pub action: String,
}
