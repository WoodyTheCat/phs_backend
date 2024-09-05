use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

use crate::error::PhsError;

use super::Session;

#[async_trait]
impl<S> FromRequestParts<S> for Session
where
    S: Sync + Send,
{
    type Rejection = PhsError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Self>().cloned().ok_or(PhsError(
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            "Can't extract session. Is `SessionManagerLayer` enabled?",
        ))
    }
}
