//! Stable HTTPS invitation URLs (roadmap C4, ADR 008, ADR 018).
//!
//! An invitation URL has the shape `{base}/invite#token={token}`. The token travels in the
//! URL fragment, which browsers never send to a server: it does not reach CloudFront or
//! Lambda access logs and is never part of a `Referer` header. The path `/invite` is stable,
//! so Android App Links can claim it (the fragment is passed to the app with the intent
//! data). The web client reads the fragment, removes it from the address bar and accepts
//! the invitation with `POST /api/invites/accept`, the token in the JSON body.

use std::fmt;

use einkaufio_application::InvitationToken;

/// Path of the web/App Links invitation route below the public base URL.
pub const INVITATION_PATH: &str = "/invite";

/// Environment variable holding the public base URL of the web client.
pub const PUBLIC_APP_BASE_URL_ENV: &str = "PUBLIC_APP_BASE_URL";

/// The validated public HTTPS origin (optionally with a path prefix) of the web client.
#[derive(Clone, Eq, PartialEq)]
pub struct InvitationBaseUrl(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationBaseUrlError {
    Missing,
    NotHttps,
    MissingHost,
    /// Query strings, fragments, credentials, whitespace and control characters are refused.
    InvalidCharacters,
}

impl InvitationBaseUrl {
    const MAX_LENGTH: usize = 256;

    pub fn try_new(value: &str) -> Result<Self, InvitationBaseUrlError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(InvitationBaseUrlError::Missing);
        }
        let rest = value
            .strip_prefix("https://")
            .ok_or(InvitationBaseUrlError::NotHttps)?;
        if value.len() > Self::MAX_LENGTH
            || value
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '?' | '#' | '@' | '\\'))
        {
            return Err(InvitationBaseUrlError::InvalidCharacters);
        }
        let host = rest.split('/').next().unwrap_or_default();
        if host.is_empty() || host.starts_with(':') {
            return Err(InvitationBaseUrlError::MissingHost);
        }
        Ok(Self(value.trim_end_matches('/').to_owned()))
    }

    /// Reads [`PUBLIC_APP_BASE_URL_ENV`]. The composition root must abort startup on error.
    pub fn from_env() -> Result<Self, InvitationBaseUrlError> {
        let value =
            std::env::var(PUBLIC_APP_BASE_URL_ENV).map_err(|_| InvitationBaseUrlError::Missing)?;
        Self::try_new(&value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The URL handed to the invitation creator. It contains the bearer secret: never log it.
    pub(crate) fn invitation_url(&self, token: &InvitationToken) -> String {
        format!(
            "{}{INVITATION_PATH}#token={}",
            self.0,
            token.expose_secret()
        )
    }
}

impl fmt::Debug for InvitationBaseUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("InvitationBaseUrl")
            .field(&self.0)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn builds_a_stable_https_url_with_the_token_in_the_fragment() {
        let base = InvitationBaseUrl::try_new("https://app.example.test/").unwrap();
        let token = InvitationToken::try_new(SECRET).unwrap();
        assert_eq!(
            base.invitation_url(&token),
            format!("https://app.example.test/invite#token={SECRET}")
        );
        let prefixed = InvitationBaseUrl::try_new("https://example.test/einkaufio").unwrap();
        assert_eq!(
            prefixed.invitation_url(&token),
            format!("https://example.test/einkaufio/invite#token={SECRET}")
        );
    }

    #[test]
    fn rejects_insecure_or_ambiguous_base_urls() {
        for (value, error) in [
            ("", InvitationBaseUrlError::Missing),
            ("http://app.example.test", InvitationBaseUrlError::NotHttps),
            ("app.example.test", InvitationBaseUrlError::NotHttps),
            ("https://", InvitationBaseUrlError::MissingHost),
            ("https:///path", InvitationBaseUrlError::MissingHost),
            (
                "https://app.example.test/?a=b",
                InvitationBaseUrlError::InvalidCharacters,
            ),
            (
                "https://app.example.test/#x",
                InvitationBaseUrlError::InvalidCharacters,
            ),
            (
                "https://user@app.example.test",
                InvitationBaseUrlError::InvalidCharacters,
            ),
            (
                "https://app.example.test/a b",
                InvitationBaseUrlError::InvalidCharacters,
            ),
        ] {
            assert_eq!(InvitationBaseUrl::try_new(value), Err(error), "{value}");
        }
    }
}
