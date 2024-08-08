#![warn(clippy::all, nonstandard_style, missing_debug_implementations)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use self::{
    service::{CookieController, PlaintextCookie, SessionManager, SessionManagerLayer},
    session::{Expiry, Id, IdType, Session, SessionData},
    store::{SessionStore, SessionStoreError},
};

mod extract;
mod service;
mod session;
mod session_store;
mod store;

/// Session errors.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Maps `serde_json` errors.
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    /// Maps `session_store::Error` errors.
    #[error(transparent)]
    Store(#[from] SessionStoreError),
}

type Result<T> = std::result::Result<T, Error>;
