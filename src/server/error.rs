//! The one error type every tool returns, carrying a category the caller can
//! act on rather than a sentence it has to read.

use std::fmt;

use rmcp::model::{ContentBlock, IntoContents};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
// `Fork` is only ever produced by the fork group, but the vocabulary a caller
// sees should not change with the build.
#[cfg_attr(not(feature = "fork"), allow(dead_code))]
pub enum ErrorCategory {
    /// The arguments were wrong. Nothing was sent upstream.
    InvalidInput,
    /// No network is configured under that name.
    UnknownNetwork,
    /// The network is configured, but this tool needs something it does not
    /// have: an explorer instance, a trace namespace, a price anchor.
    UnsupportedOnNetwork,
    /// The upstream took longer than the request timeout.
    UpstreamTimeout,
    /// The upstream answered, and the answer was an error or unparseable.
    UpstreamResponse,
    /// A local Anvil fork failed to start, died, or is not known.
    Fork,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolError {
    pub category: ErrorCategory,
    pub message: String,
    /// Present when the caller can do something about it, such as the list of
    /// networks that do have an explorer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

impl ToolError {
    fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            remedy: None,
        }
    }

    pub fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::InvalidInput, message)
    }

    pub fn unknown_network(name: &str, configured: &[String]) -> Self {
        Self::new(
            ErrorCategory::UnknownNetwork,
            format!("No network named {name:?} is configured."),
        )
        .with_remedy(format!("Configured networks: {}.", configured.join(", ")))
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::UnsupportedOnNetwork, message)
    }

    pub fn upstream_timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::UpstreamTimeout, message)
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::UpstreamResponse, message)
    }

    #[cfg_attr(not(feature = "fork"), allow(dead_code))]
    pub fn fork(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Fork, message)
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.category, self.message)?;
        match &self.remedy {
            Some(remedy) => write!(f, " {remedy}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ToolError {}

impl IntoContents for ToolError {
    fn into_contents(self) -> Vec<ContentBlock> {
        let text = serde_json::to_string_pretty(&self).unwrap_or_else(|_| self.to_string());
        vec![ContentBlock::text(text)]
    }
}

impl From<reqwest::Error> for ToolError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            return Self::upstream_timeout(format!("Upstream request timed out: {error}"));
        }
        Self::upstream(format!("Upstream request failed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_category_as_snake_case() {
        let error = ToolError::unsupported("no explorer");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["category"], "unsupported_on_network");
    }

    #[test]
    fn omits_absent_remedy() {
        let json = serde_json::to_value(ToolError::invalid_input("bad")).unwrap();
        assert!(json.get("remedy").is_none());
    }

    #[test]
    fn unknown_network_lists_the_alternatives() {
        let error = ToolError::unknown_network("base", &["mainnet".into(), "sepolia".into()]);
        assert_eq!(error.category, ErrorCategory::UnknownNetwork);
        assert!(error.remedy.unwrap().contains("mainnet, sepolia"));
    }
}
