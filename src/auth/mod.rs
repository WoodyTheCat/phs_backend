use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

use serde::{Deserialize, Serialize};

use crate::sessions::Session;
use crate::{error::PhsError, resources::Role};

mod endpoints;
mod permission;
mod service;

pub use endpoints::router;
pub use permission::{Group, Permission, RequirePermission};
pub use service::AuthManagerLayer;

#[async_trait]
impl<S> FromRequestParts<S> for AuthSession {
    type Rejection = PhsError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .ok_or(PhsError(
                StatusCode::UNAUTHORIZED,
                None,
                "No auth session found in request extensions",
            ))
            .cloned()
    }
}

#[derive(Clone)]
pub struct AuthSession {
    session: Session,
    auth_user: AuthUser,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthUser {
    id: i32,

    username: String,
    hash: String,

    permissions: Vec<Permission>,
    role: Role,
    groups: Vec<String>,
}

impl AuthUser {
    pub const fn id(&self) -> i32 {
        self.id
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }
}

impl<'a> AuthSession {
    pub fn session(&self) -> Session {
        self.session.clone()
    }

    pub async fn destroy(&mut self) -> Result<(), PhsError> {
        self.session.flush().await.map_err(Into::into)
    }

    pub const fn data(&self) -> &AuthUser {
        &self.auth_user
    }

    pub async fn from_session(session: Session) -> Result<Option<Self>, PhsError> {
        let s = session
            .get()
            .await?
            .map(|auth_user| Self { session, auth_user });

        Ok(s)
    }
}
