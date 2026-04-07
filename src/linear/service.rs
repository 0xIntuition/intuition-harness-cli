mod assets;
mod catalog;
mod issues;
mod resolution;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod workpad;

use anyhow::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LinearService<C> {
    pub(super) client: C,
    pub(super) default_team: Option<String>,
}

impl<C> LinearService<C> {
    pub fn new(client: C, default_team: Option<String>) -> Self {
        Self {
            client,
            default_team,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearFailureKind {
    Transient,
    Authentication,
    Permission,
    Configuration,
    Other,
}

impl LinearFailureKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Authentication => "authentication",
            Self::Permission => "permission",
            Self::Configuration => "configuration",
            Self::Other => "other",
        }
    }

    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Transient)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearFailure {
    pub kind: LinearFailureKind,
    pub message: String,
    #[serde(default)]
    pub status_code: Option<u16>,
}

impl LinearFailure {
    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }
}

pub fn classify_linear_failure(error: &Error) -> LinearFailure {
    let message = error.to_string();
    let normalized_messages = error
        .chain()
        .map(|cause| cause.to_string().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let normalized = normalized_messages.join(" | ");
    let status_code = normalized_messages
        .iter()
        .find_map(|message| extract_linear_status_code(message));

    let kind = if normalized.contains("failed to reach the linear graphql endpoint")
        || normalized.contains("failed to read the linear response body")
        || normalized.contains("failed to decode the linear response payload")
        || normalized.contains("timed out")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || matches!(
            status_code,
            Some(408 | 409 | 425 | 429 | 500 | 502 | 503 | 504)
        )
        || normalized.contains("rate limit")
        || normalized.contains("too many requests")
    {
        LinearFailureKind::Transient
    } else if matches!(status_code, Some(401))
        || contains_any(
            &normalized,
            &[
                "unauthorized",
                "authentication failed",
                "invalid api key",
                "invalid token",
            ],
        )
    {
        LinearFailureKind::Authentication
    } else if matches!(status_code, Some(403))
        || contains_any(
            &normalized,
            &[
                "forbidden",
                "permission denied",
                "access denied",
                "not authorized",
                "insufficient permissions",
            ],
        )
    {
        LinearFailureKind::Permission
    } else if matches!(status_code, Some(400 | 404 | 422))
        || contains_any(
            &normalized,
            &[
                "invalid input",
                "was not found on team",
                "issue `",
                "could not find referenced issue",
                "linear returned no data",
            ],
        )
    {
        LinearFailureKind::Configuration
    } else {
        LinearFailureKind::Other
    };

    LinearFailure {
        kind,
        message,
        status_code,
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn extract_linear_status_code(message: &str) -> Option<u16> {
    let index = message.find("status ")?;
    let digits = message[index + "status ".len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() >= 3 {
        digits.parse::<u16>().ok()
    } else {
        None
    }
}
