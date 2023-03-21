use anyhow::anyhow;
use axum::headers::{self, Header, HeaderName, HeaderValue};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};
use std::fmt::{Display, Formatter};

/// Implement WebSub signatures:
/// https://repl.ca/what-is-x-hub-signature/
/// https://www.w3.org/TR/websub/#signing-content
#[derive(Clone, Debug)]
pub struct XHubSignature {
    pub algorithm: XHubSignatureAlgorithm,
    signature: Vec<u8>,
}

impl XHubSignature {
    pub fn is_valid(&self, secret: &[u8], body: &[u8]) -> bool {
        match self.algorithm {
            XHubSignatureAlgorithm::Sha1 => {
                let mut mac =
                    Hmac::<Sha1>::new_from_slice(secret).expect("HMAC can take key of any size");
                mac.update(body);
                mac.verify_slice(&self.signature).is_ok()
            }
            XHubSignatureAlgorithm::Sha256 => {
                let mut mac =
                    Hmac::<Sha256>::new_from_slice(secret).expect("HMAC can take key of any size");
                mac.update(body);
                mac.verify_slice(&self.signature).is_ok()
            }
            XHubSignatureAlgorithm::Sha384 => {
                let mut mac =
                    Hmac::<Sha384>::new_from_slice(secret).expect("HMAC can take key of any size");
                mac.update(body);
                mac.verify_slice(&self.signature).is_ok()
            }
            XHubSignatureAlgorithm::Sha512 => {
                let mut mac =
                    Hmac::<Sha512>::new_from_slice(secret).expect("HMAC can take key of any size");
                mac.update(body);
                mac.verify_slice(&self.signature).is_ok()
            }
        }
    }
}

static X_HUB_SIGNATURE: HeaderName = HeaderName::from_static("x-hub-signature");

impl Header for XHubSignature {
    fn name() -> &'static HeaderName {
        &X_HUB_SIGNATURE
    }

    fn decode<'i, I>(values: &mut I) -> std::result::Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue>,
    {
        let Some(value) = values.next() else {
            return Err(headers::Error::invalid());
        };
        if values.next().is_some() {
            // There should not be more than one of these headers.
            return Err(headers::Error::invalid());
        }
        let parts = value
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .split('=')
            .collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err(headers::Error::invalid());
        }
        let algorithm =
            XHubSignatureAlgorithm::try_from(parts[0]).map_err(|_| headers::Error::invalid())?;
        let signature = hex::decode(parts[1]).map_err(|_| headers::Error::invalid())?;
        if signature.len() != algorithm.signature_length() {
            return Err(headers::Error::invalid());
        }
        Ok(Self {
            algorithm,
            signature,
        })
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let signature = hex::encode(&self.signature);
        let algorithm: &str = self.algorithm.into();
        let formatted = format!("{algorithm}={signature}");
        if let Ok(header_value) = HeaderValue::from_str(&formatted) {
            values.extend([header_value].into_iter());
        }
    }
}

/// https://www.w3.org/TR/websub/#recognized-algorithm-names
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum XHubSignatureAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl XHubSignatureAlgorithm {
    fn signature_length(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

impl TryFrom<&str> for XHubSignatureAlgorithm {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> anyhow::Result<Self> {
        match value {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            "sha384" => Ok(Self::Sha384),
            "sha512" => Ok(Self::Sha512),
            _ => Err(anyhow!("Unknown X-Hub-Signature algorithm: {value}")),
        }
    }
}

impl From<XHubSignatureAlgorithm> for &str {
    fn from(value: XHubSignatureAlgorithm) -> Self {
        match value {
            XHubSignatureAlgorithm::Sha1 => "sha1",
            XHubSignatureAlgorithm::Sha256 => "sha256",
            XHubSignatureAlgorithm::Sha384 => "sha384",
            XHubSignatureAlgorithm::Sha512 => "sha512",
        }
    }
}

impl Display for XHubSignatureAlgorithm {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str((*self).into())
    }
}
