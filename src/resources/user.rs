use std::fmt::Debug;

use argon2::{
    password_hash,
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{prelude::FromRow, PgPool};
use tracing::instrument;

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
    resources::Department,
};

use super::{
    CursorOptions, CursorPaginatable, CursorResponse, HasSqlxQueryString, SqlxQueryString,
};

pub fn router() -> Router {
    Router::new()
        .route("/v1/users", get(get_users).post(create_user))
        .route(
            "/v1/users/:id",
            get(get_user).put(put_user).delete(delete_user),
        )
        .route("/v1/users/change-password", post(change_password))
        .route("/v1/users/reset-password", post(reset_password))
}

#[derive(Serialize, Deserialize, sqlx::Type, Debug, Clone, Copy, PartialEq, Eq)]
#[sqlx(type_name = "role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Teacher,
    Admin,
    Student,
}

#[derive(Serialize, Deserialize, FromRow)]
struct User {
    id: i32,
    username: String,
    name: String,

    description: String,
    department: Option<i32>,

    role: Role,
    permissions: Vec<Permission>,
}

impl HasSqlxQueryString for User {
    type QueryString = UserQueryString;
}

#[derive(Debug, Deserialize)]
struct UserQueryString {
    id: Option<i32>,
    username: Option<String>,
    name: Option<String>,

    #[serde(default, with = "::serde_with::rust::double_option")]
    department: Option<Option<i32>>,
    role: Option<Role>,

    sort_by: Option<String>,
}

impl SqlxQueryString for UserQueryString {
    fn where_clause<'a>(&'a self, builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>) {
        if let Some(id) = self.id {
            builder.push(" AND id = ");
            builder.push_bind(id);
        }

        if let Some(ref name) = self.name {
            builder.push(" AND name LIKE ");
            builder.push_bind(name);
        }

        if let Some(ref username) = self.username {
            builder.push(" AND username LIKE ");
            builder.push_bind(username);
        }

        if let Some(department) = self.department {
            builder.push(" AND department = ");
            builder.push_bind(department);
        }

        if let Some(role) = self.role {
            builder.push(" AND role = ");
            builder.push_bind(role);
        }
    }

    fn order_by_clause<'a>(&'a self, builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>) -> bool {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            return false;
        };

        if let s @ ("id" | "name" | "modified" | "created_at" | "updated_at") = field.as_str() {
            builder.push(s);
            order.append_to(builder);
            true
        } else {
            false
        }
    }
}

impl CursorPaginatable for User {
    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Deserialize, Debug)]
struct CreateUserRequest {
    name: String,
    username: String,
    password: String,
    role: Role,
    description: String,
    department: Option<i32>,
}

#[instrument(skip(pool, _auth_session, req))]
async fn create_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<User>, PhsError> {
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
            StatusCode::NOT_FOUND,
            None,
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
            None,
            "A user with this username already exists",
        ));
    }

    let hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &SaltString::generate(&mut OsRng))?
        .to_string();

    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, username, role, description, department, hash)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id,
            name,
            username,
            role as "role: _",
            description,
            department,
            permissions as "permissions: _"
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

#[instrument(skip(pool))]
async fn get_user(
    Path(id): Path<i32>,
    Extension(pool): Extension<PgPool>,
) -> Result<Json<User>, PhsError> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT id, name, username, role as "role: Role", description, department, permissions as "permissions: Vec<Permission>"
        FROM users
        WHERE id = $1
        "#,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(user))
}

#[instrument(skip(pool, _auth_session))]
async fn get_users(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as u8 }>,

    Query(cursor_options): Query<CursorOptions>,
    Query(query_string): Query<<User as HasSqlxQueryString>::QueryString>,

    Extension(pool): Extension<PgPool>,
) -> Result<Json<CursorResponse<User>>, PhsError> {
    let users_no_hash = super::paginated_query_as::<User>(
        r#"SELECT id, name, username, role, description, department, permissions FROM users"#,
        cursor_options,
        query_string,
        &pool,
    )
    .await?;

    Ok(Json(CursorResponse::new(users_no_hash)))
}

#[derive(Deserialize, Debug)]
struct PutUserBody {
    username: Option<String>,
    name: Option<String>,
    description: Option<String>,
    department: Option<i32>,
    role: Option<Role>,
}

#[instrument(skip(pool, _auth_session))]
async fn put_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as u8 }>,

    Path(id): Path<i32>,
    Extension(pool): Extension<PgPool>,
    Json(body): Json<PutUserBody>,
) -> Result<Json<User>, PhsError> {
    let user_no_hash = sqlx::query_as!(
        User,
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
            role as "role: _",
            permissions as "permissions: _"
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

#[instrument(skip(pool, redis_pool, auth_session, body))]
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
            password_hash::Error::Password => {
                PhsError(StatusCode::UNAUTHORIZED, Some(Box::new(e)), "Unauthorised")
            }
            e => e.into(),
        })?;

    // Authenticated as of here

    let new_hash = Argon2::default()
        .hash_password(
            body.new_password.as_bytes(),
            &SaltString::generate(&mut OsRng),
        )?
        .to_string();

    sqlx::query!(
        r#"
        UPDATE users
        SET hash = $1
        WHERE users.id = $2
        "#,
        new_hash,
        user_data.id()
    )
    .execute(&pool)
    .await?;

    let mut conn = redis_pool.get().await?;

    let mut sessions: Vec<String> = redis::cmd("FT.SEARCH")
        .arg("idx:sessionsUserId")
        .arg(format!(r#""@id:[{0} {0}]""#, user_data.id()))
        .arg("NOCONTENT")
        .query_async(&mut conn)
        .await
        .unwrap();

    // Clear all of the user's other sessions

    let current_session_id = hex::encode(Sha256::digest(
        auth_session
            .session()
            .get_hashed_id()
            .await
            .ok_or(PhsError(
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
                "Error getting hashed session ID",
            ))?,
    ));

    let Some(current_index) = sessions.iter().position(|s| *s == current_session_id) else {
        return Err(PhsError(
            StatusCode::UNAUTHORIZED,
            None,
            "Current session not found in user's sessions, most likely an expiry",
        ));
    };

    // Remove the current session from the list to delete
    sessions.swap_remove(current_index);

    redis::cmd("DEL")
        .arg(sessions)
        .exec_async(&mut conn)
        .await?;

    Ok(())
}

#[derive(Deserialize)]
struct PostResetPasswordBody {
    user_id: i32,
    new_password: String,
}

#[instrument(skip_all)]
async fn reset_password(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as u8 }>,

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

    sqlx::query!(
        r#"
        UPDATE users
        SET hash = $1
        WHERE users.id = $2
        "#,
        new_hash,
        body.user_id
    )
    .execute(&pool)
    .await?;

    // Clear all of the user's sessions
    let mut conn = redis_pool.get().await?;

    let sessions: Vec<String> = redis::cmd("FT.SEARCH")
        .arg("idx:sessionsUserId")
        .arg(format!(r#""@id:[{0} {0}]""#, body.user_id))
        .arg("NOCONTENT")
        .query_async(&mut conn)
        .await?;

    redis::cmd("DEL")
        .arg(sessions)
        .exec_async(&mut conn)
        .await?;

    Ok(())
}

#[instrument(skip(pool, _auth_session))]
async fn delete_user(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManageUsers as u8 }>,

    Path(id): Path<i32>,
    Extension(pool): Extension<PgPool>,
) -> Result<(), PhsError> {
    sqlx::query!("DELETE FROM users WHERE id = $1", id)
        .execute(&pool)
        .await?;

    Ok(())
}
