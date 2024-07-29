use std::fmt::Debug;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use axum::{extract::Path, http::StatusCode, routing::get, Extension, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool};

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
};

use super::Department;

#[derive(Serialize, Deserialize, sqlx::Type, Debug, Clone, PartialEq, Eq)]
#[sqlx(type_name = "role", rename_all = "lowercase")]
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

    pub description: String,
    pub department: Option<i32>,

    pub role: Role,
    pub permissions: Vec<Permission>,
}

impl Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("username", &self.username)
            .field("role", &self.role)
            .field("description", &self.description)
            .field("department", &self.department)
            .finish()
    }
}

#[derive(Serialize, Deserialize, FromRow)]
pub struct UserHash {
    pub id: i32,
    pub hash: String,
}

pub fn router() -> Router {
    Router::new()
        .route("/v1/users", get(get_users).post(create_user))
        .route("/v1/users/:id", get(get_user))
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
) -> Result<Json<PublicUser>, PhsError> {
    if req.department.is_some()
        && sqlx::query_as!(
            Department,
            r#"SELECT * FROM departments WHERE id = $1"#,
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
        PublicUser,
        r#"
        INSERT INTO users (name, username, role, description, department, hash)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, name, username, role as "role: Role", description, department
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
struct PublicUser {
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
) -> Result<Json<PublicUser>, PhsError> {
    let user = sqlx::query_as!(
        PublicUser,
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

async fn get_users(Extension(pool): Extension<PgPool>) -> Result<Json<Vec<PublicUser>>, PhsError> {
    let user = sqlx::query_as!(
        PublicUser,
        r#"
        SELECT id, name, username, role as "role: Role", description, department
        FROM users
        "#,
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(user))
}
