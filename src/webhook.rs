use mastodon_async::entities::{
    admin::{Account, Report},
    status::Status,
};
use serde::{Deserialize, Serialize};
use time::{serde::iso8601, OffsetDateTime};

/// Parameters for request to our webhook handler.
#[derive(Deserialize)]
pub struct Params {
    #[serde(default)]
    pub domain: Option<String>,
}

/// The events that can be received by our webhook handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event")]
pub enum Event {
    #[serde(rename = "account.approved")]
    AccountApproved {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        account: Account,
    },
    #[serde(rename = "account.created")]
    AccountCreated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        account: Account,
    },
    #[serde(rename = "account.updated")]
    AccountUpdated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        account: Account,
    },
    #[serde(rename = "report.created")]
    ReportCreated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        report: Report,
    },
    #[serde(rename = "report.updated")]
    ReportUpdated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        report: Report,
    },
    #[serde(rename = "status.created")]
    StatusCreated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        status: Status,
    },
    #[serde(rename = "status.updated")]
    StatusUpdated {
        #[serde(with = "iso8601")]
        created_at: OffsetDateTime,
        #[serde(rename = "object")]
        status: Status,
    },
    #[serde(other)]
    Unknown,
}
