#![warn(
    clippy::all,
    nonstandard_style,
    future_incompatible,
    missing_debug_implementations
)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use tower_cookies::cookie;
pub use tower_sessions_core::{session, session_store};
#[doc(inline)]
pub use tower_sessions_core::{
    session::{Expiry, Id, IdType, Session, SessionData},
    session_store::{ExpiredDeletion, SessionStore}, // CachingSessionStore,
};

pub use crate::service::{SessionManager, SessionManagerLayer};

pub mod service;
