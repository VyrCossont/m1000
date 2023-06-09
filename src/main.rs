mod config;
mod event;
mod interop;
mod pattern;
mod setup;
mod webhook;
mod websub;

use crate::config::{Config, Report, Restrict, Rule, Settings, USER_AGENT};
use crate::event::report::handle_report;
use crate::event::status::handle_status;
use crate::interop::mime::dump_as_mime;
use crate::pattern::{CompileMatcher, RuleMatcher};
use crate::setup::{
    ensure_config, ensure_mastodon, ensure_registered, ensure_settings, ensure_webhook, setup,
};
use crate::websub::{XHubSignature, XHubSignatureAlgorithm};
use anyhow::{anyhow, bail, Error, Result};
use axum::body::Bytes;
use axum::extract::{Query, TypedHeader};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Router};
use clap::{Parser, Subcommand};
use futures::stream::{FuturesUnordered, StreamExt};
use mastodon_async::Mastodon;
use reqwest::Client;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config_dir = &cli.config_dir;

    let client = &Client::builder().user_agent(USER_AGENT).build()?;

    return match cli.command {
        Command::Setup {
            ref domain,
            ref username,
        } => setup(config_dir, client, domain, username).await,
        Command::Serve => serve(config_dir, client).await,
        Command::Healthcheck => healthcheck(config_dir, client).await,
        Command::DumpAsMime {
            ref domain,
            ref username,
            ref id,
        } => dump_as_mime(config_dir, client, domain, username, id).await,
    };
}

async fn serve(config_dir: &PathBuf, client: &Client) -> Result<()> {
    let domain_handler_map = init_domain_handlers(config_dir, &client).await?;

    let make_service = Router::new()
        .route("/healthcheck", get(serve_healthcheck))
        .route("/webhook", post(receive_webhook))
        .layer(Extension(Arc::new(Mutex::new(domain_handler_map))))
        .into_make_service();

    let settings = ensure_settings(config_dir)?;
    let server_futures = FuturesUnordered::new();
    for addr_str in settings.listen {
        let addr = SocketAddr::from_str(&addr_str)?;
        info!("Listening on {}", addr);
        let server_future = axum::Server::bind(&addr).serve(make_service.clone());
        server_futures.push(server_future);
    }
    for server_result in server_futures.collect::<Vec<_>>().await {
        server_result?;
    }

    Ok(())
}

/// Healthcheck command suitable for Docker. Calls our healthcheck endpoint on the first listen address.
async fn healthcheck(config_dir: &PathBuf, client: &Client) -> Result<()> {
    let settings = ensure_settings(config_dir)?;
    let addr_str = settings.listen.first().ok_or(anyhow!(
        "Couldn't find any listen addresses in global settings"
    ))?;
    let status = client
        .get(format!("http://{addr_str}/healthcheck"))
        .send()
        .await?
        .status();
    if !status.is_success() {
        bail!("Health check request failed: {status}");
    }
    Ok(())
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Root config directory.
    #[arg(short, long)]
    config_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run interactive setup.
    Setup {
        /// Domain name of the instance to which you're connecting.
        #[arg(short, long)]
        domain: String,
        /// Username of the bot account you're using, without the leading @ or domain.
        #[arg(short, long)]
        username: String,
    },
    /// Run the server.
    Serve,
    /// Try to call our own health check endpoint.
    Healthcheck,
    /// Testing: Dump a post as a MIME message.
    DumpAsMime {
        /// Domain name of the instance to which you're connecting.
        #[arg(short, long)]
        domain: String,
        /// Username of the bot account you're using, without the leading @ or domain.
        #[arg(short, long)]
        username: String,
        /// ID of the post to dump.
        #[arg(short, long)]
        id: String,
    },
}

/// Process a healthcheck request.
/// Currently, we have no real state to check, so we always return a 2xx response.
async fn serve_healthcheck() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// Arbitrary.
const EVENT_CHANNEL_SIZE: usize = 256;

/// Initialize domain handlers:
/// - create per-domain webhook event channels
/// - ensure that this app is registered with each domain
/// - ensure that this app's user credentials are valid for each domain user
/// - compile rule patterns to matchers
/// - spawn a task to handle each domain user's webhook events
async fn init_domain_handlers(
    config_dir: &PathBuf,
    client: &Client,
) -> Result<HashMap<String, DomainHandler>> {
    let settings = ensure_settings(config_dir)?;
    let mut domain_handler_map = HashMap::<String, DomainHandler>::new();
    let domains_and_usernames = config::configured_domains_and_usernames(config_dir)?;
    for (domain, usernames) in domains_and_usernames {
        let webhook = ensure_webhook(config_dir, &domain, false)?;
        let webhook_secret = webhook.secret.bytes().collect();
        let (event_sender, _) = broadcast::channel::<webhook::Event>(EVENT_CHANNEL_SIZE);
        info!(
            "Webhook ready for {webhook_domain}",
            webhook_domain = webhook.domain
        );

        let registered = ensure_registered(config_dir, client, &domain).await?;

        for username in usernames {
            let mastodon =
                ensure_mastodon(config_dir, registered.clone(), &domain, &username, false).await?;

            let account = mastodon.verify_credentials().await?;

            info!(
                "Authenticated with {username}@{domain}",
                username = account.username,
                domain = domain,
            );

            let config =
                CompiledConfig::try_from(&ensure_config(config_dir, &domain, &username).await?)?;

            tokio::spawn(handle_events(
                event_sender.subscribe(),
                settings.clone(),
                config,
                mastodon,
            ));
        }

        domain_handler_map.insert(
            domain.clone(),
            DomainHandler {
                domain,
                webhook_secret,
                event_sender,
            },
        );
    }

    Ok(domain_handler_map)
}

/// Holds the webhook secret and event channel sender for one domain.
/// The sender may fan out to multiple users under that domain.
#[derive(Clone, Debug)]
struct DomainHandler {
    domain: String,
    webhook_secret: Vec<u8>,
    event_sender: broadcast::Sender<webhook::Event>,
}

/// Same as [`Config`] but with compiled rules.
#[derive(Clone, Debug)]
pub struct CompiledConfig {
    pub domain: String,
    pub username: String,
    pub rules: Vec<CompiledRule>,
}

impl TryFrom<&Config> for CompiledConfig {
    type Error = Error;

    fn try_from(config: &Config) -> Result<Self> {
        let mut rules = vec![];
        for rule in config.rules.iter() {
            rules.push(CompiledRule::try_from(rule)?);
        }
        Ok(Self {
            domain: config.domain.clone(),
            username: config.username.clone(),
            rules,
        })
    }
}

/// Same as [`Rule`] but with patterns compiled to matchers.
#[derive(Clone, Debug)]
pub struct CompiledRule {
    pub name: String,
    pub report: Option<Report>,
    pub restrict: Option<Restrict>,
    pub matchers: Vec<RuleMatcher>,
}

impl TryFrom<&Rule> for CompiledRule {
    type Error = Error;

    fn try_from(rule: &Rule) -> Result<Self> {
        let mut matchers = vec![];
        for pattern in rule.patterns.iter() {
            matchers.push(pattern.compile()?);
        }
        Ok(Self {
            name: rule.name.clone(),
            report: rule.report.clone(),
            restrict: rule.restrict.clone(),
            matchers,
        })
    }
}

/// Receive a webhook event, figure out which domain it's for, and route it to the right domain handler.
async fn receive_webhook(
    Extension(domain_handler_map): Extension<Arc<Mutex<HashMap<String, DomainHandler>>>>,
    TypedHeader(x_hub_signature): TypedHeader<XHubSignature>,
    Query(params): Query<webhook::Params>,
    body: Bytes,
) -> StatusCode {
    if x_hub_signature.algorithm != XHubSignatureAlgorithm::Sha256 {
        // Mastodon supports exactly one signature algorithm.
        error!(
            "Unsupported webhook signature algorithm: {algorithm}",
            algorithm = x_hub_signature.algorithm
        );
        return StatusCode::UNAUTHORIZED;
    }

    let (domain, event_sender) = {
        let domain_handler_map = domain_handler_map.lock().await;
        let domain_handlers: Vec<&DomainHandler>;
        if let Some(domain) = params.domain {
            domain_handlers = domain_handler_map.get(&domain).into_iter().collect();
        } else {
            // Try to find a domain handler for which this signature would be valid.
            domain_handlers = domain_handler_map.values().collect();
        }
        let matching_domain_handlers = domain_handlers
            .into_iter()
            .filter(|domain_handler| {
                x_hub_signature.is_valid(&domain_handler.webhook_secret, &body)
            })
            .collect::<Vec<_>>();
        if matching_domain_handlers.len() > 1 {
            error!("Multiple domains could have signed an incoming webhook event");
            return StatusCode::UNAUTHORIZED;
        }
        let Some(domain_handler) = matching_domain_handlers.first() else {
            error!("Could not find a domain that could have signed an incoming webhook event");
            return StatusCode::UNAUTHORIZED;
        };
        (
            domain_handler.domain.clone(),
            domain_handler.event_sender.clone(),
        )
    };

    match serde_json::from_slice::<webhook::Event>(&body) {
        Err(e) => {
            error!(
                "{domain}: Decoding error {e}: {body}",
                body = String::from_utf8_lossy(&body)
            );
            StatusCode::UNPROCESSABLE_ENTITY
        }
        Ok(event) => {
            if let Err(e) = event_sender.send(event) {
                error!("{domain}: Channel error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::ACCEPTED
            }
        }
    }
}

/// Handle webhook events for a given domain user.
async fn handle_events(
    mut event_receiver: broadcast::Receiver<webhook::Event>,
    settings: Settings,
    config: CompiledConfig,
    mastodon: Mastodon,
) -> Result<()> {
    let domain = &config.domain;
    let username = &config.username;
    loop {
        match event_receiver.recv().await {
            Ok(event) => match event {
                webhook::Event::StatusCreated { status, .. }
                | webhook::Event::StatusUpdated { status, .. } => {
                    if let Err(e) = handle_status(&settings, &config, &mastodon, &status).await {
                        error!("{username}@{domain}: Error handling status: {e}");
                    }
                }
                webhook::Event::ReportCreated { report, .. }
                | webhook::Event::ReportUpdated { report, .. } => {
                    if let Err(e) = handle_report(&settings, &domain, &report).await {
                        error!("{username}@{domain}: Error handling status: {e}");
                    }
                }
                _ => {
                    info!("{username}@{domain}: Unimplemented event type: {event:#?}");
                }
            },
            Err(RecvError::Lagged(skipped)) => {
                error!("{username}@{domain}: Channel error: fell behind event stream. Skipping {skipped} events to catch up.");
            }
            Err(RecvError::Closed) => {
                return Ok(());
            }
        }
    }
}
