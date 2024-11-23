use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub tls_enabled: bool,
    pub tls_options: Option<TlsOptions>,
    #[cfg(debug_assertions)]
    pub use_tokio_console: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsOptions {
    pub key_path: PathBuf,
    pub cert_path: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            https_port: 443,
            http_port: 80,
            tls_enabled: false,
            tls_options: None,
            #[cfg(debug_assertions)]
            use_tokio_console: false,
        }
    }
}

impl ServerConfig {
    pub fn get_cert_filepath(&self) -> Option<&PathBuf> {
        if let (true, Some(TlsOptions { ref cert_path, .. })) =
            (self.tls_enabled, &self.tls_options)
        {
            Some(cert_path)
        } else {
            None
        }
    }

    pub fn get_key_filepath(&self) -> Option<&PathBuf> {
        if let (true, Some(TlsOptions { ref key_path, .. })) = (self.tls_enabled, &self.tls_options)
        {
            Some(key_path)
        } else {
            None
        }
    }
}
