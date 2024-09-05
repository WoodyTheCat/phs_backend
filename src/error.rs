use std::fmt::Debug;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::sessions;

#[derive(Debug)]
pub struct PhsError(pub StatusCode, pub Option<Box<dyn Debug>>, pub &'static str);

impl IntoResponse for PhsError {
    fn into_response(self) -> Response {
        tracing::error!(error = ?self.1, "Error {}: {}:", self.0, self.2);

        // Return the canonical reason to remain ambiguous about system workings
        (self.0, self.0.canonical_reason().unwrap()).into_response()
    }
}

impl From<sqlx::Error> for PhsError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(e = %e, "Sqlx Error");
        match e {
            sqlx::Error::RowNotFound => Self(
                StatusCode::NOT_FOUND,
                Some(Box::new(e)),
                "The requested resource was not found",
            ),
            _ => Self(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(Box::new(e)),
                "SqlX error",
            ),
        }
    }
}

impl From<argon2::password_hash::Error> for PhsError {
    fn from(e: argon2::password_hash::Error) -> Self {
        tracing::error!(e = %e, "Argon2id Error");
        match e {
            argon2::password_hash::Error::Password => Self(
                StatusCode::UNAUTHORIZED,
                Some(Box::new(e)),
                "Incorrect or invalid password",
            ),
            _ => Self(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(Box::new(e)),
                "Argon2 error",
            ),
        }
    }
}

impl From<tokio::task::JoinError> for PhsError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Tokio join error",
        )
    }
}

impl From<sessions::Error> for PhsError {
    fn from(e: sessions::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Sessions error",
        )
    }
}

impl From<(StatusCode, &'static str)> for PhsError {
    fn from(e: (StatusCode, &'static str)) -> Self {
        Self(e.0, Some(Box::new(e)), e.1)
    }
}

impl From<deadpool_redis::PoolError> for PhsError {
    fn from(e: deadpool_redis::PoolError) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Redis pool error",
        )
    }
}

impl From<redis::RedisError> for PhsError {
    fn from(e: redis::RedisError) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Redis error",
        )
    }
}

impl From<tokio::io::Error> for PhsError {
    fn from(e: tokio::io::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Tokio IO error",
        )
    }
}

impl From<serde_json::Error> for PhsError {
    fn from(e: serde_json::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Box::new(e)),
            "Error whilst serialising or deserialising JSON",
        )
    }
}
