use crate::config::Rspamd;
use crate::interop::mime;
use anyhow::{bail, Result};
use mastodon_async::prelude::Status;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Run a MIME message version of a post through rspamd, returning the action it recommends.
pub async fn rspamd_scan(rspamd: &Rspamd, domain: &str, status: &Status) -> Result<String> {
    let rspamc_output: RspamcSymbolsOutput =
        rspamc_command(rspamd, "symbols", domain, status).await?;
    Ok(rspamc_output.action)
}

/// Tell rspamd to learn a MIME message version of a post as ham.
pub async fn rspamd_learn_ham(rspamd: &Rspamd, domain: &str, status: &Status) -> Result<()> {
    let _rspamc_output: RspamcLearnOutput =
        rspamc_command(rspamd, "learn_ham", domain, status).await?;
    Ok(())
}

/// Tell rspamd to learn a MIME message version of a post as spam.
pub async fn rspamd_learn_spam(rspamd: &Rspamd, domain: &str, status: &Status) -> Result<()> {
    let _rspamc_output: RspamcLearnOutput =
        rspamc_command(rspamd, "learn_spam", domain, status).await?;
    Ok(())
}

async fn rspamc_command<'de, T: DeserializeOwned>(
    rspamd: &Rspamd,
    command_name: &str,
    domain: &str,
    status: &Status,
) -> Result<T> {
    let Some(rspamc_command) = rspamd.rspamc_command.as_ref() else {
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
    command.arg(command_name);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    let mut process = command.spawn()?;

    let Some(mut stdin) = process.stdin.take() else {
        bail!("Couldn't get rspamc stdin");
    };
    let message_builder = mime::status_to_mime(domain, &status);
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

    let deserialized_output: T = serde_json::from_slice(json_slice)?;
    Ok(deserialized_output)
}

/// JSON output of `rspamc` or synonym `rspamc symbols`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RspamcSymbolsOutput {
    /// Action recommended. The most common are `no action` and `reject`, but there are others:
    /// https://rspamd.com/doc/faq.html#what-are-rspamd-actions
    pub action: String,
}

/// JSON output of `rspamc learn_ham` or synonym `rspamc learn_spam`.
/// No significant fields yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RspamcLearnOutput {}
