use std::fmt::{Debug, Display};

use argon2::{
    password_hash,
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use axum::{
    debug_handler,
    extract::{Path, Query},
    http::StatusCode,
    routing::get,
    Extension, Json, Router,
};
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{prelude::FromRow, PgPool};

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
};

use super::Department;

#[derive(Serialize, Deserialize, sqlx::Type, Debug, Clone, PartialEq, Eq)]
#[sqlx(type_name = "role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Teacher,
    Admin,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct User {
    pub id: i32,
    pub username: String,
    pub hash: String, // A PHC-format hash string of the user's password

    pub name: String,
    pub role: Role,

    pub description: String,
    pub department: Option<i32>,

    pub permissions: Vec<Permission>,
}

impl Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "User {}", self.id)
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/v1/users", get(get_users).post(create_user))
        .route(
            "/v1/users/:id",
            get(get_user).put(put_user).delete(delete_user),
        )
        .route("/v1/users/change_password", get(change_password))
        .route("/v1/users/reset_password", get(reset_password))
}

#[derive(Deserialize)]
struct CreateUserRequest {
    name: String,
    username: String,
    password: String,
    role: Role,
    description: String,
    department: Option<i32>,
}

async fn create_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserSafe>, PhsError> {
    if req.department.is_some()
        && sqlx::query_as!(
            Department,
            r#"SELECT id, department FROM departments WHERE id = $1"#,
            req.department.unwrap()
        )
        .fetch_optional(&pool)
        .await?
        .is_none()
    {
        return Err(PhsError(
            StatusCode::BAD_REQUEST,
            "No department exists with this ID",
        ));
    }

    if sqlx::query!(r#"SELECT id FROM users WHERE username = $1"#, req.username)
        .fetch_optional(&pool)
        .await?
        .is_some()
    {
        return Err(PhsError(
            StatusCode::BAD_REQUEST,
            "A user with this username already exists",
        ));
    }

    let hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &SaltString::generate(&mut OsRng))?
        .to_string();

    let user = sqlx::query_as!(
        UserSafe,
        r#"
        INSERT INTO users (name, username, role, description, department, hash)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id,
            name,
            username,
            role as "role: _",
            description,
            department
        "#,
        req.name,
        req.username,
        req.role as Role,
        req.description,
        req.department,
        hash
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(user))
}

#[derive(Serialize)]
struct UserSafe {
    id: i32,
    name: String,
    username: String,
    role: Role,
    description: String,
    department: Option<i32>,
}

async fn get_user(
    Path(id): Path<i32>,
    Extension(pool): Extension<PgPool>,
) -> Result<Json<UserSafe>, PhsError> {
    let user = sqlx::query_as!(
        UserSafe,
        r#"
        SELECT id, name, username, role as "role: Role", description, department
        FROM users
        WHERE id = $1
        "#,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(user))
}

#[derive(Serialize)]
struct UserNoHash {
    id: i32,
    username: String,
    name: String,

    description: String,
    department: Option<i32>,

    role: Role,
    permissions: Vec<Permission>,
}

async fn get_users(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    Extension(pool): Extension<PgPool>,
) -> Result<Json<Vec<UserNoHash>>, PhsError> {
    let users_no_hash = sqlx::query_as!(
        UserNoHash,
        r#"
        SELECT id,
            name,
            username,
            role as "role: _",
            description,
            department,
            permissions as "permissions: _"
        FROM users
        "#,
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(users_no_hash))
}

#[derive(Deserialize)]
struct PutUserBody {
    username: Option<String>,
    name: Option<String>,
    description: Option<String>,
    department: Option<i32>,
    role: Option<Role>,
}

async fn put_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    Query(id): Query<i32>,
    Extension(pool): Extension<PgPool>,
    Json(body): Json<PutUserBody>,
) -> Result<Json<UserSafe>, PhsError> {
    let user_no_hash = sqlx::query_as!(
        UserSafe,
        r#"
        UPDATE users SET
            username = $1,
            name = $2,
            description = $3,
            department = $4,
            role = $5
        WHERE id = $6
        RETURNING id,
            username,
            name,
            description,
            department,
            role as "role: _"
        "#,
        body.username,
        body.name,
        body.description,
        body.department,
        body.role as Option<Role>,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(user_no_hash))
}

#[derive(Deserialize)]
struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

// TODO: Implement a way to completely log out a user -> Delete multiple from RedisPool
#[debug_handler]
async fn change_password(
    auth_session: AuthSession,
    Extension(pool): Extension<PgPool>,
    Extension(redis_pool): Extension<RedisPool>,
    Json(body): Json<ChangePasswordBody>,
) -> Result<(), PhsError> {
    let user_data = auth_session.data();

    Argon2::default()
        .verify_password(
            body.current_password.as_bytes(),
            &PasswordHash::new(user_data.hash())?,
        )
        .map_err(|e| match e {
            password_hash::Error::Password => PhsError::UNAUTHORIZED,
            _ => PhsError::INTERNAL,
        })?;

    // Verified as of here

    let new_hash = Argon2::default()
        .hash_password(
            body.new_password.as_bytes(),
            &SaltString::generate(&mut OsRng),
        )?
        .to_string();

    // TODO: Clear all sessions other than this one

    sqlx::query!(
        r#"
        UPDATE users
        SET hash = $1
        WHERE users.id = $2
        "#,
        new_hash,
        user_data.id()
    )
    .fetch_one(&pool)
    .await?;

    let mut conn = redis_pool.get().await?;

    let sessions: Vec<String> = redis::cmd("FT.SEARCH")
        .arg("idx:sessionsUserId")
        .arg(format!(r#""@id:[{0} {0}]""#, user_data.id()))
        .query_async(&mut conn)
        .await
        .unwrap();

    let current_session_id = hex::encode(Sha256::digest(
        auth_session
            .session()
            .get_hashed_id()
            .await
            .ok_or(PhsError::UNAUTHORIZED)?,
    ));

    let exclusive_sessions: Vec<String> = sessions
        .into_iter()
        .filter(|s| s != &current_session_id)
        .collect();

    redis::cmd("DEL")
        .arg(exclusive_sessions)
        .exec_async(&mut conn)
        .await
        .map_err(|_| PhsError::INTERNAL)?;

    Ok(())
}

#[derive(Deserialize)]
struct PostResetPasswordBody {
    user_id: i32,
    new_password: String,
}

async fn reset_password(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    Extension(pool): Extension<PgPool>,
    Extension(redis_pool): Extension<RedisPool>,

    Json(body): Json<PostResetPasswordBody>,
) -> Result<(), PhsError> {
    let new_hash = Argon2::default()
        .hash_password(
            body.new_password.as_bytes(),
            &SaltString::generate(&mut OsRng),
        )?
        .to_string();

    // TODO: Clear all sessions other than this one

    sqlx::query!(
        r#"
        UPDATE users
        SET hash = $1
        WHERE users.id = $2
        "#,
        new_hash,
        body.user_id
    )
    .fetch_one(&pool)
    .await?;

    let mut conn = redis_pool.get().await?;

    let sessions: Vec<String> = redis::cmd("FT.SEARCH")
        .arg("idx:sessionsUserId")
        .arg(format!(r#""@id:[{0} {0}]""#, body.user_id))
        .query_async(&mut conn)
        .await
        .unwrap();

    redis::cmd("DEL")
        .arg(sessions)
        .exec_async(&mut conn)
        .await
        .map_err(|_| PhsError::INTERNAL)?;

    Ok(())
}

async fn delete_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    Query(id): Query<i32>,
    Extension(pool): Extension<PgPool>,
) -> Result<(), PhsError> {
    sqlx::query!("delete from users where id = $1", id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}
