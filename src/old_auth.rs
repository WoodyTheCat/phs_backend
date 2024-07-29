use std::collections::HashSet;

use argon2::{Argon2, PasswordHash, PasswordVerifier};
// use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    //     async_trait,
    //     extract::FromRequestParts,
    response::IntoResponse,
    routing::{get, post},
    Extension,
    Json,
    Router, //     response::IntoResponse,
            //     routing::{get, post},
            //     Extension, Json, Router,
};
// use serde::{Deserialize, Serialize};
// use sqlx::PgPool;
// use tower_sessions::Session;

// use crate::{
//     error::PhsError,
//     resources::{Role, User},
// };

mod middleware;
mod service;

pub use service::{AuthBackend, AuthManager, AuthManagerLayer, AuthSession};

pub use axum;
use axum::async_trait;
pub use backend::{AuthUser, AuthnBackend, AuthzBackend};
pub use permissions::Permission;
use serde::Deserialize;
pub use session::AuthSession;
use sqlx::{prelude::FromRow, PgPool};

use crate::{
    error::PhsError,
    resources::{Role, User},
};

pub struct AuthRequired;

// #[async_trait]
// impl<S> FromRequestParts<S> for AuthRequired {
//     type Rejection = PhsError;

//     async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
//         let session = parts.extensions.get::<AuthSession>();

//         Ok(())
//     }
// }

impl AuthUser for User {
    fn id(&self) -> i32 {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.hash.as_bytes()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct Backend {
    db: PgPool,
}

impl Backend {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuthnBackend for Backend {
    async fn authenticate(&self, creds: Credentials) -> Result<User, PhsError> {
        let maybe_user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE username = ? ")
            .bind(creds.username)
            .fetch_optional(&self.db)
            .await?;

        let Some(user) = maybe_user else {
            return Err(PhsError::UNAUTHORIZED);
        };

        tokio::task::spawn_blocking(move || {
            Argon2::default()
                .verify_password(
                    creds.password.as_bytes(),
                    &PasswordHash::new(user.hash.as_str())?,
                )
                .map_err(|e| match e {
                    argon2::password_hash::Error::Password => PhsError::UNAUTHORIZED,
                    _ => PhsError::INTERNAL,
                })?;

            Ok(user)
        })
        .await?
    }

    async fn get_user(&self, user_id: &i32) -> Result<Option<User>, PhsError> {
        let user = sqlx::query_as!(User, r#"SELECT id, name, username, role as "role: Role", description, department, hash FROM users WHERE id = $1"#, user_id)
            .fetch_optional(&self.db)
            .await?;

        Ok(user)
    }
}

type UserSession = AuthSession<Backend>;

// #[derive(Debug, Clone, Eq, PartialEq, Hash, FromRow)]
// pub struct Permission {
//     pub name: String,
// }

// impl From<&str> for Permission {
//     fn from(name: &str) -> Self {
//         Permission {
//             name: name.to_string(),
//         }
//     }
// }

// #[async_trait]
// impl AuthzBackend for Backend {
//     type Permission = Permission;

//     async fn get_group_permissions(
//         &self,
//         user: &Self::User,
//     ) -> Result<HashSet<Self::Permission>, PhsError> {
//         let permissions: Vec<Self::Permission> = sqlx::query_as!(
//             Vec<Self::Permission>,
//             r#"
//             SELECT DISTINCT permissions.name
//             FROM users
//             JOIN users_groups ON users.id = users_groups.user_id
//             JOIN groups_permissions ON users_groups.group_id = groups_permissions.group_id
//             JOIN permissions ON groups_permissions.permission_id = permissions.id
//             WHERE users.id = $1
//             "#,
//             user.id,
//         )
//         .fetch_all(&self.db)
//         .await?;

//         Ok(permissions.into_iter().collect())
//     }
// }

// pub fn router() -> Router {
//     Router::new()
// }

// #[derive(Serialize, Deserialize, Default)]
// pub struct UserData {
//     pub username: String,
//     pub id: i32,
//     pub is_admin: bool,
//     pub auth_hash: Option<String>,
// }

// /// Specify if the extracted user should be an admin
// pub struct UserSession<const ADMIN: bool = false> {
//     session: Session,
//     user_data: Option<UserData>,
// }

// impl<const ADMIN: bool> UserSession<ADMIN> {
//     pub const USER_DATA_KEY: &'static str = "user.data";

//     pub async fn destroy(&mut self) -> Result<(), PhsError> {
//         self.session.delete().await?;

//         Ok(())
//     }

//     pub async fn get_data(&self) -> Option<UserData> {
//         self.session
//             .get::<UserData>(Self::USER_DATA_KEY)
//             .await
//             .unwrap()
//     }

//     pub async fn update_session(&self) {
//         self.session
//             .insert(Self::USER_DATA_KEY, &self.user_data)
//             .await
//             .unwrap();
//     }

//     pub async fn login(
//         &mut self,
//         id: i32,
//         username: String,
//         is_admin: bool,
//         auth_hash: String,
//     ) -> Result<(), PhsError> {
//         if let Some(ref data) = self.user_data {
//             if data.auth_hash.is_none() {
//                 self.session.cycle_id().await?; // Session-fixation mitigation.
//             }
//         }

//         self.user_data = Some(UserData {
//             username,
//             id,
//             is_admin,
//             auth_hash: Some(auth_hash),
//         });

//         self.update_session().await;

//         Ok(())
//     }
// }

// // impl fmt::Display for UserSession {
// //     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
// //         f.debug_struct("UserSession")
// //             .field("expiry", &self.session.expiry_date())
// //             .finish()
// //     }
// // }

// #[async_trait]
// impl<S: Send + Sync, const ADMIN: bool> FromRequestParts<S> for UserSession<ADMIN> {
//     type Rejection = PhsError;

//     async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
//         tracing::info!("Attempting to extract UserSession");
//         let session = Session::from_request_parts(req, state).await?;

//         let user_data: UserData = session
//             .get(Self::USER_DATA_KEY)
//             .await
//             .unwrap()
//             .unwrap_or_default();

//         if ADMIN && !user_data.is_admin {
//             return Err(PhsError::UNAUTHORIZED);
//         }

//         Ok(Self {
//             session,
//             user_data: Some(user_data),
//         })
//     }
// }

// // async fn handler(mut session: UserSession) -> Result<(), PhsError> {
// //     session.destroy().await?;

// //     Ok(())
// // }
