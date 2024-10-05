#![warn(clippy::all, nonstandard_style, missing_debug_implementations)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use thiserror::Error;

pub use self::{
    service::{CookieController, SessionConfig, SessionManager, SessionManagerLayer},
    session::{Expiry, IdType, Session},
    store::{SessionStore, SessionStoreError},
};

mod extract;
mod service;
mod session;
mod store;

/// Session errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Redis session store error: {0}")]
    Store(SessionStoreError),

    #[error("Session not found")]
    SessionNotFound,
}

impl From<SessionStoreError> for Error {
    fn from(value: SessionStoreError) -> Self {
        match value {
            SessionStoreError::NotFound => Self::SessionNotFound,
            e => Self::Store(e),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;
