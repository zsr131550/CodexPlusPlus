use std::fmt;

use eframe::egui;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalUrlError {
    Invalid,
}

impl fmt::Display for ExternalUrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external URL is invalid")
    }
}

impl std::error::Error for ExternalUrlError {}

#[derive(Clone, PartialEq, Eq)]
pub struct ExternalUrl(Url);

impl ExternalUrl {
    pub fn parse(value: &str) -> Result<Self, ExternalUrlError> {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ExternalUrlError::Invalid);
        }
        let (raw_scheme, authority_and_path) =
            value.split_once("://").ok_or(ExternalUrlError::Invalid)?;
        if !matches!(raw_scheme.to_ascii_lowercase().as_str(), "http" | "https")
            || authority_and_path.is_empty()
            || authority_and_path.starts_with('/')
        {
            return Err(ExternalUrlError::Invalid);
        }
        let url = Url::parse(value).map_err(|_| ExternalUrlError::Invalid)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ExternalUrlError::Invalid);
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn emit(&self, context: &egui::Context) {
        context.open_url(egui::OpenUrl::new_tab(self.as_str()));
    }
}

impl fmt::Debug for ExternalUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExternalUrl([validated])")
    }
}
