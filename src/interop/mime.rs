use crate::setup::{ensure_mastodon, ensure_registered};
use mail_builder::headers::address::{Address, EmailAddress};
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::text::Text;
use mail_builder::headers::HeaderType;
use mail_builder::MessageBuilder;
use mastodon_async::entities::StatusId;
use mastodon_async::prelude::Status;
use mastodon_async::Visibility;
use reqwest::Client;
use std::borrow::Cow;
use std::io;
use std::path::Path;

/// Dump a single post as a MIME message to stdout.
pub async fn dump_as_mime(
    config_dir: &Path,
    client: &Client,
    domain: &str,
    username: &str,
    id: &str,
) -> anyhow::Result<()> {
    let registered = ensure_registered(config_dir, client, domain).await?;
    let mastodon = ensure_mastodon(config_dir, registered, domain, username, false).await?;
    let status = mastodon.get_status(&StatusId::new(id)).await?;

    let message_builder = status_to_mime(domain, &status);
    message_builder.write_to(io::stdout())?;

    Ok(())
}

/// Translate a single post to a MIME message.
pub fn status_to_mime<'a>(domain: &str, status: &'a Status) -> MessageBuilder<'a> {
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
