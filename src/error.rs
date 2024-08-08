use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::sessions;

#[derive(Debug)]
pub struct PhsError(pub StatusCode, pub &'static str);

impl PhsError {
    pub const INTERNAL: Self = Self(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An unexpected error occurred",
    );

    pub const UNAUTHORIZED: Self = Self(StatusCode::UNAUTHORIZED, "Unauthorized");
    // pub const BAD_REQUEST: Self = Self(StatusCode::BAD_REQUEST, "Bad request");
    pub const FORBIDDEN: Self = Self(StatusCode::FORBIDDEN, "Inadequate permissions");
}

impl IntoResponse for PhsError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

impl From<sqlx::Error> for PhsError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(e = %e, "Sqlx Error");
        match e {
            sqlx::Error::RowNotFound => Self(
                StatusCode::NOT_FOUND,
                "The requested resource was not found",
            ),
            _ => Self::INTERNAL,
        }
    }
}

impl From<argon2::password_hash::Error> for PhsError {
    fn from(e: argon2::password_hash::Error) -> Self {
        tracing::error!(e = %e, "Argon2id Error");
        match e {
            argon2::password_hash::Error::Password => {
                Self(StatusCode::UNAUTHORIZED, "Incorrect or invalid password")
            }
            _ => Self::INTERNAL,
        }
    }
}

impl From<tokio::task::JoinError> for PhsError {
    fn from(e: tokio::task::JoinError) -> Self {
        tracing::error!(e = %e, "Tokio Join Error");
        Self::INTERNAL
    }
}

impl From<sessions::Error> for PhsError {
    fn from(e: sessions::Error) -> Self {
        tracing::error!(e = %e, "Sessions Error");
        Self::INTERNAL
    }
}

impl From<(StatusCode, &'static str)> for PhsError {
    fn from(e: (StatusCode, &'static str)) -> Self {
        tracing::error!("({}, {})", e.0, e.1);
        Self(e.0, e.1)
    }
}

impl From<deadpool_redis::PoolError> for PhsError {
    fn from(e: deadpool_redis::PoolError) -> Self {
        tracing::error!(?e, "Pool error");
        Self::INTERNAL
    }
}
