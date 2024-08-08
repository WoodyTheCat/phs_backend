use axum::{async_trait, extract::FromRequestParts, http::request::Parts};

use serde::{Deserialize, Serialize};

use crate::error::PhsError;
use crate::sessions::Session;

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
            .get::<AuthSession>()
            .ok_or(PhsError::UNAUTHORIZED)
            .cloned()
    }
}

#[derive(Clone)]
pub struct AuthSession {
    session: Session,
    auth_user: AuthUser,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AuthUser {
    id: i32,

    username: String,
    hash: String,

    permissions: Vec<Permission>,
    groups: Vec<String>,
}

impl AuthUser {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    pub fn groups(&self) -> &[String] {
        &self.groups
    }
}

impl<'a> AuthSession {
    pub fn session(&self) -> Session {
        self.session.clone()
    }

    pub async fn destroy(&mut self) -> Result<(), PhsError> {
        self.session.flush().await.map_err(Into::into)
    }

    pub fn data(&self) -> &AuthUser {
        &self.auth_user
    }

    pub async fn from_session(session: Session) -> Result<Option<Self>, PhsError> {
        let s = session
            .get()
            .await?
            .map(|auth_user| Self { auth_user, session });

        Ok(s)

        // Skip this part if we clear sessions when deleting a user WARN TODO

        // let user = backend
        //     .get_user(session_user.id)
        //     .await?
        //     .ok_or(PhsError::UNAUTHORIZED)?;

        // And this part if we clear the session when the password is changed WARN TODO

        // let hashes_match: bool = user
        //     .hash
        //     .as_bytes()
        //     .ct_eq(session_user.hash.as_bytes())
        //     .into();

        // if !hashes_match {
        //     session.flush().await?;
        // }

        // Ok(Self {
        //     user: session_user,
        //     session,
        //     user_key,
        // })
    }
}

// impl AuthUser for User {
//     fn id(&self) -> i32 {
//         self.id
//     }

//     fn session_auth_hash(&self) -> &[u8] {
//         self.hash.as_bytes()
//     }
// }

// #[derive(Debug, Clone, Deserialize)]
// pub struct Credentials {
//     pub username: String,
//     pub password: String,
// }

// #[derive(Debug, Clone)]
// pub struct Backend {
//     db: PgPool,
// }

// impl Backend {
//     pub fn new(db: PgPool) -> Self {
//         Self { db }
//     }
// }

// #[async_trait]
// impl AuthnBackend for Backend {
//     async fn authenticate(&self, creds: Credentials) -> Result<User, PhsError> {
//         let maybe_user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE username = ? ")
//             .bind(creds.username)
//             .fetch_optional(&self.db)
//             .await?;

//         let Some(user) = maybe_user else {
//             return Err(PhsError::UNAUTHORIZED);
//         };

//         tokio::task::spawn_blocking(move || {
//             Argon2::default()
//                 .verify_password(
//                     creds.password.as_bytes(),
//                     &PasswordHash::new(user.hash.as_str())?,
//                 )
//                 .map_err(|e| match e {
//                     argon2::password_hash::Error::Password => PhsError::UNAUTHORIZED,
//                     _ => PhsError::INTERNAL,
//                 })?;

//             Ok(user)
//         })
//         .await?
//     }

//     async fn get_user(&self, user_id: &i32) -> Result<Option<User>, PhsError> {
//         let user = sqlx::query_as!(User, r#"SELECT id, name, username, role as "role: Role", description, department, hash FROM users WHERE id = $1"#, user_id)
//             .fetch_optional(&self.db)
//             .await?;

//         Ok(user)
//     }
// }

// type UserSession = AuthSession<Backend>;

// pub fn router() -> Router {
//     Router::new()
//         .route("/v1/login", post(login))
//         .route("/v1/logout", get(logout))
//         .route("/v1/whoami", get(whoami))
// }

// #[derive(Deserialize)]
// struct PostLoginBody {
//     username: String,
//     password: String,
// }

// async fn login(
//     mut auth_session: UserSession,
//     Extension(pool): Extension<PgPool>,
//     Json(credentials): Json<PostLoginBody>,
// ) -> Result<impl IntoResponse, PhsError> {
//     tracing::info!("Logging in");

//     let user = sqlx::query_as!(
//         User,
//         r#"
//         SELECT id, name, username, role as "role: Role", description, department, hash
//         FROM users
//         WHERE username = $1
//         "#,
//         credentials.username
//     )
//     .fetch_one(&pool)
//     .await?;

//     Argon2::default()
//         .verify_password(
//             credentials.password.as_bytes(),
//             &PasswordHash::new(user.hash.as_str())?,
//         )
//         .map_err(|e| match e {
//             argon2::password_hash::Error::Password => PhsError::UNAUTHORIZED,
//             _ => PhsError::INTERNAL,
//         })?;

//     // Correct credentials

//     if auth_session
//         .login(user, user.username, user.role == Role::Admin, user.hash)
//         .await
//         .is_err()
//     {
//         return Err(PhsError::INTERNAL);
//     }

//     Ok("Logged in!")
// }

// async fn whoami(session: UserSession) -> Result<String, PhsError> {
//     Ok(session
//         .get_data()
//         .await
//         .map(|d| d.username)
//         .unwrap_or("Not logged in!".into()))
// }

// async fn logout(
//     mut auth_session: UserSession,
//     // Extension(pool): Extension<PgPool>,
// ) -> Result<(), PhsError> {
//     tracing::info!("Logging out");

//     auth_session.destroy().await?;

//     Ok(())
// }
