use crate::config::*;
use anyhow::{bail, Result};
use mastodon_async::data::Data;
use mastodon_async::entities::auth::Scopes;
use mastodon_async::mastodon::Mastodon;
use mastodon_async::registration::{Registered, Registration};
use reqwest::Client;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use tracing::info;

/// Run interactive setup for a single domain and username.
pub async fn setup(config_dir: &Path, client: &Client, domain: &str, username: &str) -> Result<()> {
    let _ = ensure_settings(config_dir);
    let _ = ensure_webhook(config_dir, domain, true);
    let registered = ensure_registered(config_dir, client, domain).await?;
    let _ = ensure_mastodon(config_dir, registered, domain, username, true).await?;
    let _ = ensure_config(config_dir, domain, username).await?;
    Ok(())
}

/// Load settings or use and save defaults.
pub fn ensure_settings(config_dir: &Path) -> Result<Settings> {
    if let Ok(settings) = Settings::load(config_dir) {
        return Ok(settings);
    }

    let settings = Settings {
        listen: vec![
            SocketAddr::new(IpAddr::from(Ipv6Addr::UNSPECIFIED), DEFAULT_PORT).to_string(),
        ],
        rspamc_command: find_rspamc(),
    };
    settings.save(config_dir)?;
    info!(
        "Global settings saved: {path}",
        path = settings.path(config_dir).to_string_lossy(),
    );

    Ok(settings)
}

/// Find `rspamc`, if installed.
#[cfg(unix)]
fn find_rspamc() -> Option<Vec<String>> {
    let output = Command::new("which").arg("rspamc").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?.trim().to_string();
    Some(vec![path])
}

/// `rspamc` probably doesn't exist on Windows.
#[cfg(not(unix))]
fn find_rspamc() -> Option<Vec<String>> {
    None
}

/// Load an existing webhook signing secret if there is one,
/// or prompt the user to provide one, then save it.
pub fn ensure_webhook(config_dir: &Path, domain: &str, interactive: bool) -> Result<Webhook> {
    if let Ok(webhook) = Webhook::load(config_dir, domain) {
        return Ok(webhook);
    }

    if !interactive {
        bail!(
            "You need to configure a webhook for {domain}. \
            Run `{client_name} setup` to finish setup.",
            client_name = CLIENT_NAME
        );
    }

    let mut secret = String::new();
    println!("Webhook signing secret for {domain}:");
    let _ = std::io::stdin().read_line(&mut secret)?;
    secret = secret.trim().to_string();

    let webhook = Webhook {
        domain: domain.to_string(),
        secret,
    };
    webhook.save(config_dir)?;
    info!(
        "Webhook secret saved for {domain}: {path}",
        path = webhook.path(config_dir,).to_string_lossy(),
    );

    Ok(webhook)
}

/// Load an existing registered app if there is one, or register and save a new one.
pub async fn ensure_registered(
    config_dir: &Path,
    client: &Client,
    domain: &str,
) -> Result<Registered> {
    let base = format!("https://{domain}");

    if let Ok(app) = App::load(config_dir, domain) {
        return Ok(Registered::from_parts(
            &base,
            &app.client_id,
            &app.client_secret,
            OOB_REDIRECT_URL,
            Scopes::from_str(REQUIRED_SCOPES.join(" ").as_str())?,
            false,
        ));
    }

    let registered = Registration::new_with_client(base, client.clone())
        .scopes(Scopes::from_str(&REQUIRED_SCOPES.join(" "))?)
        .client_name(CLIENT_NAME)
        .website(CLIENT_WEBSITE)
        .force_login(true)
        .build()
        .await?;

    let (_, client_id, client_secret, _, scopes, _) = registered.clone().into_parts();
    let app = App {
        domain: domain.to_string(),
        client_id,
        client_secret,
        scopes,
    };
    app.save(config_dir)?;
    info!(
        "OAuth application for {client_name} registered and saved for {domain}: {path}",
        client_name = CLIENT_NAME,
        path = app.path(config_dir,).to_string_lossy(),
    );

    Ok(registered)
}

/// Load an existing access token if there is one,
/// or prompt the user to authenticate and get one, then save it.
pub async fn ensure_mastodon(
    config_dir: &Path,
    registered: Registered,
    domain: &str,
    username: &str,
    interactive: bool,
) -> Result<Mastodon> {
    if let Ok(credentials) = Credentials::load(config_dir, &domain.clone(), &username.clone()) {
        let (_, client_id, client_secret, _, _, _) = registered.clone().into_parts();
        return Ok(Mastodon::from(Data {
            base: format!("https://{domain}").into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect: OOB_REDIRECT_URL.into(),
            token: credentials.access_token.into(),
        }));
    }

    if !interactive {
        bail!(
            "You need to authenticate the bot user account {username}@{domain}. \
            Run `{client_name} setup` to finish setup.",
            client_name = CLIENT_NAME
        );
    }

    let mastodon = loop {
        let authorize_url = registered.authorize_url()?;
        println!("Authorization URL: {authorize_url}");

        let mut auth_code = String::new();
        println!("Authorization code:");
        let _ = std::io::stdin().read_line(&mut auth_code)?;
        auth_code = auth_code.trim().to_string();

        let mastodon = registered.complete(auth_code).await?;

        let account = mastodon.verify_credentials().await?;
        if account.acct == username {
            // Implies `account.username == username` and the domain is the local one.
            // This is the correct account.
            break mastodon;
        }

        eprintln!(
            "Expected authorization for {username}@{domain}, got {acct} instead. \
            Please try again and make sure you're logging into the correct account.",
            acct = account.acct
        );
    };

    let credentials = Credentials {
        domain: domain.to_string(),
        username: username.to_string(),
        access_token: mastodon.data.token.to_string(),
    };
    credentials.save(config_dir)?;
    info!(
        "Access token saved for {username}@{domain}: {path}",
        path = credentials.path(config_dir,).to_string_lossy(),
    );

    Ok(mastodon)
}

/// Load an existing config file if there is one,
/// or create and save a demo config.
pub async fn ensure_config(config_dir: &Path, domain: &str, username: &str) -> Result<Config> {
    if let Ok(config) = Config::load(config_dir, domain, username) {
        return Ok(config);
    }

    let config = Config {
        domain: domain.to_string(),
        username: username.to_string(),
        rules: vec![Rule {
            name: "no orange website".to_string(),
            report: Some(Report {
                rule_ids: vec![],
                spam: false,
                forward: false,
            }),
            restrict: None,
            patterns: vec![RulePattern::Post {
                post: PostPattern::Text {
                    text: TextPattern::Link {
                        link: LinkPattern::Domain {
                            domain: "news.ycombinator.com".to_string(),
                        },
                    },
                },
            }],
        }],
    };
    config.save(config_dir)?;
    info!(
        "Empty default configuration created for {username}@{domain}: {path}",
        path = config.path(config_dir,).to_string_lossy(),
    );

    Ok(config)
}
