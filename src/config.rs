use anyhow::{bail, Result};
use glob::glob;
use mastodon_async::entities::auth::Scopes;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::path::{Path, PathBuf};

pub const CLIENT_NAME: &str = env!("CARGO_PKG_NAME");

pub const CLIENT_WEBSITE: &str = env!("CARGO_PKG_HOMEPAGE");

pub const USER_AGENT: &str = {
    concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION"),
        "; ",
        env!("CARGO_PKG_HOMEPAGE"),
    )
};

pub const OOB_REDIRECT_URL: &str = "urn:ietf:wg:oauth:2.0:oob";

pub const REQUIRED_SCOPES: &[&str] = &["read", "write", "push", "admin:read", "admin:write"];

pub const DEFAULT_PORT: u16 = 1337;

// TODO: implement JSON/YAML schema dump for config files.
/// Schemas for types that don't have them.
mod schema {
    use schemars::gen::SchemaGenerator;
    use schemars::schema::{Schema, SchemaObject};
    use schemars::JsonSchema;

    pub fn scopes(gen: &mut SchemaGenerator) -> Schema {
        let mut schema: SchemaObject = <String>::json_schema(gen).into();
        schema.format = Some("scopes".to_owned());
        schema.into()
    }
}

/// Global settings for this program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    /// Addresses and ports to listen on.
    pub listen: Vec<String>,
    /// Rspamc command. May be a single path or executable name, or an ssh, docker, etc. command in several parts.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rspamc_command: Option<Vec<String>>,
}

impl StoredOnce for Settings {}

/// A registered OAuth application for a given domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct App {
    pub domain: String,
    pub client_id: String,
    pub client_secret: String,
    #[schemars(schema_with = "schema::scopes")]
    pub scopes: Scopes,
}

impl StoredPerDomain for App {}

/// A webhook secret for a given domain.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Webhook {
    pub domain: String,
    pub secret: String,
}

impl StoredPerDomain for Webhook {}

/// Access token for a given user and domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Credentials {
    pub domain: String,
    pub username: String,
    pub access_token: String,
}

impl StoredPerDomainUser for Credentials {}

/// Moderation rules for a given user and domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Config {
    pub domain: String,
    pub username: String,
    pub rules: Vec<Rule>,
}

impl StoredPerDomainUser for Config {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub name: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Report>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restrict: Option<Restrict>,
    pub patterns: Vec<RulePattern>,
}

/// If this is present, the rule will send a report using this metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Report {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rule_ids: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Clone::clone")]
    pub spam: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "Clone::clone")]
    pub forward: bool,
}

/// If this is present, the rule will restrict the relevant account.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Restrict {
    Sensitive,
    Disable,
    Silence,
    Suspend,
}

/// Top level pattern for a rule that matches against a post or the account that created it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum RulePattern {
    Account { account: AccountPattern },
    Post { post: PostPattern },
    Rspamd { action: String },
    Any { any: Vec<RulePattern> },
    All { all: Vec<RulePattern> },
    Not { not: Box<RulePattern> },
}

/// Patterns that match against an account's username/domain or bio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum AccountPattern {
    User { user: UserPattern },
    Text { text: TextPattern },
    Any { any: Vec<AccountPattern> },
    All { all: Vec<AccountPattern> },
    Not { not: Box<AccountPattern> },
}

/// Patterns that match against the content of a post.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum PostPattern {
    Text { text: TextPattern },
    Any { any: Vec<PostPattern> },
    All { all: Vec<PostPattern> },
    Not { not: Box<PostPattern> },
}

/// Patterns that apply to HTML content with optional metadata (mentions and hashtags).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum TextPattern {
    Word { word: String },
    Regex { regex: String },
    Link { link: LinkPattern },
    Mention { mention: UserPattern },
    Hashtag { hashtag: StringPattern },
    Any { any: Vec<TextPattern> },
    All { all: Vec<TextPattern> },
    Not { not: Box<TextPattern> },
}

/// Patterns that apply to the username or domain of an account or mention.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum UserPattern {
    Username { username: StringPattern },
    Instance { instance: InstancePattern },
    Local { local: bool },
    Any { any: Vec<UserPattern> },
    All { all: Vec<UserPattern> },
    Not { not: Box<UserPattern> },
}

/// Patterns that apply to any string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum StringPattern {
    Word { word: String },
    Regex { regex: String },
    Any { any: Vec<StringPattern> },
    All { all: Vec<StringPattern> },
    Not { not: Box<StringPattern> },
}

/// Patterns that apply to the URL of any link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum LinkPattern {
    Word { word: String },
    Regex { regex: String },
    Domain { domain: String },
    Any { any: Vec<LinkPattern> },
    All { all: Vec<LinkPattern> },
    Not { not: Box<LinkPattern> },
}

/// Patterns that apply to an instance's domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum InstancePattern {
    Word { word: String },
    Regex { regex: String },
    Domain { domain: String },
    Any { any: Vec<InstancePattern> },
    All { all: Vec<InstancePattern> },
    Not { not: Box<InstancePattern> },
}

pub trait StoredOnce: private::StoredOnce {
    fn load(config_dir: &Path) -> Result<Self> {
        load_from(<Self as private::StoredOnce>::path(config_dir))
    }

    fn path(&self, config_dir: &Path) -> PathBuf {
        <Self as private::StoredOnce>::path(config_dir)
    }

    fn save(&self, config_dir: &Path) -> Result<()> {
        save_to(self, self.path(config_dir))
    }
}

pub trait StoredPerDomain: private::StoredPerDomain {
    fn load(config_dir: &Path, domain: &str) -> Result<Self> {
        let data: Self = load_from(<Self as private::StoredPerDomain>::path(config_dir, domain))?;
        if data.domain() != domain {
            bail!(
                "Expected domain {domain}, got {data_domain}",
                data_domain = data.domain()
            );
        }
        Ok(data)
    }

    fn path(&self, config_dir: &Path) -> PathBuf {
        <Self as private::StoredPerDomain>::path(config_dir, self.domain())
    }

    fn save(&self, config_dir: &Path) -> Result<()> {
        save_to(self, self.path(config_dir))
    }
}

pub trait StoredPerDomainUser: private::StoredPerDomainUser {
    fn load(config_dir: &Path, domain: &str, username: &str) -> Result<Self> {
        let data: Self = load_from(<Self as private::StoredPerDomainUser>::path(
            config_dir, domain, username,
        ))?;
        if data.domain() != domain {
            bail!(
                "Expected domain {domain}, got {data_domain}",
                data_domain = data.domain()
            );
        }
        if data.username() != username {
            bail!(
                "Expected username {username}, got {data_username}",
                data_username = data.username()
            );
        }
        Ok(data)
    }

    fn path(&self, config_dir: &Path) -> PathBuf {
        <Self as private::StoredPerDomainUser>::path(config_dir, self.domain(), self.username())
    }

    fn save(&self, config_dir: &Path) -> Result<()> {
        save_to(self, self.path(config_dir))
    }
}

/// There's no easy way to have private trait methods in Rust, so we split these traits.
pub(crate) mod private {
    use super::{App, Config, Credentials, Settings, Webhook};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::path::{Path, PathBuf};

    pub trait StoredOnce: DeserializeOwned + Serialize {
        fn basename() -> &'static str;

        fn path(config_dir: &Path) -> PathBuf {
            config_dir
                .to_path_buf()
                .join(format!("{basename}.yaml", basename = Self::basename()))
        }
    }

    impl StoredOnce for Settings {
        fn basename() -> &'static str {
            "global"
        }
    }

    pub trait StoredPerDomain: DeserializeOwned + Serialize {
        fn basename() -> &'static str;
        fn domain(&self) -> &str;

        fn path(config_dir: &Path, domain: &str) -> PathBuf {
            config_dir
                .to_path_buf()
                .join(domain)
                .join(format!("{basename}.yaml", basename = Self::basename()))
        }
    }

    impl StoredPerDomain for App {
        fn basename() -> &'static str {
            "app"
        }
        fn domain(&self) -> &str {
            self.domain.as_str()
        }
    }

    impl StoredPerDomain for Webhook {
        fn basename() -> &'static str {
            "webhook"
        }
        fn domain(&self) -> &str {
            self.domain.as_str()
        }
    }

    pub trait StoredPerDomainUser: DeserializeOwned + Serialize {
        fn basename() -> &'static str;
        fn domain(&self) -> &str;
        fn username(&self) -> &str;

        fn path(config_dir: &Path, domain: &str, username: &str) -> PathBuf {
            config_dir
                .to_path_buf()
                .join(domain)
                .join(username)
                .join(format!("{basename}.yaml", basename = Self::basename()))
        }
    }

    impl StoredPerDomainUser for Credentials {
        fn basename() -> &'static str {
            "credentials"
        }
        fn domain(&self) -> &str {
            self.domain.as_str()
        }
        fn username(&self) -> &str {
            self.username.as_str()
        }
    }

    impl StoredPerDomainUser for Config {
        fn basename() -> &'static str {
            "config"
        }
        fn domain(&self) -> &str {
            self.domain.as_str()
        }
        fn username(&self) -> &str {
            self.username.as_str()
        }
    }
}

fn load_from<T: DeserializeOwned>(path: PathBuf) -> Result<T> {
    let file = File::open(path)?;
    let data = serde_yaml::from_reader(file)?;
    Ok(data)
}

fn save_to<T>(data: &T, path: PathBuf) -> Result<()>
where
    T: Serialize,
{
    if let Some(dir) = path.parent() {
        create_dir_all(dir)?;
    }
    let file = File::create(path)?;
    serde_yaml::to_writer(file, data)?;
    Ok(())
}

/// Map of configured domains and bot account usernames associated with them.
pub fn configured_domains_and_usernames(config_dir: &Path) -> Result<HashMap<String, Vec<String>>> {
    let mut domains_to_usernames = HashMap::new();

    let mut webhook_glob_buf = config_dir.to_path_buf();
    webhook_glob_buf.push("*");
    webhook_glob_buf.push(format!(
        "{basename}.yaml",
        basename = <Webhook as private::StoredPerDomain>::basename()
    ));
    let Some(webhook_glob) = webhook_glob_buf.to_str() else {
        bail!(
            "{glob} couldn't be converted to a string for globbing",
            glob = webhook_glob_buf.to_string_lossy()
        );
    };

    for webhook_entry in glob(webhook_glob)? {
        let webhook = load_from::<Webhook>(webhook_entry?)?;
        let domain = webhook.domain;

        let mut usernames = Vec::new();

        let mut config_glob_buf = config_dir.to_path_buf();
        config_glob_buf.push(&domain);
        config_glob_buf.push("*");
        config_glob_buf.push(format!(
            "{basename}.yaml",
            basename = <Config as private::StoredPerDomainUser>::basename()
        ));
        let Some(config_glob) = config_glob_buf.to_str() else {
            bail!(
                "{glob} couldn't be converted to a string for globbing",
                glob = config_glob_buf.to_string_lossy()
            );
        };

        for config_entry in glob(config_glob)? {
            let config = load_from::<Config>(config_entry?)?;
            let username = config.username;
            usernames.push(username);
        }

        domains_to_usernames.insert(domain, usernames);
    }

    Ok(domains_to_usernames)
}
