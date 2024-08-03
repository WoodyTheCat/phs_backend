use argon2::{password_hash, Argon2, PasswordHash, PasswordVerifier};
use axum::{
    debug_handler,
    extract::Query,
    response::IntoResponse,
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower_sessions::{IdType, Session};

use crate::{auth::Permission, error::PhsError, resources::User};

use super::{AuthSession, Group, RequirePermission};

pub fn router() -> Router {
    Router::new()
        .route("/v1/login", post(login))
        .route("/v1/logout", get(logout))
        .route("/v1/whoami", get(whoami))
        .route("/v1/groups", get(get_groups).post(create_group))
        .route("/v1/group/:id", put(put_group).delete(delete_group))
        .route(
            "/v1/user_groups",
            get(add_to_group).delete(delete_from_group),
        )
}

#[derive(Deserialize)]
struct PostLoginBody {
    username: String,
    password: String,
}

#[debug_handler]
async fn login(
    session: Session,
    Extension(pool): Extension<PgPool>,
    Json(credentials): Json<PostLoginBody>,
) -> Result<impl IntoResponse, PhsError> {
    tracing::info!("Logging in");

    let user = sqlx::query_as!(
        User,
        r#"
        SELECT id, name, username, role as "role: _", description, department, hash, permissions as "permissions: Vec<Permission>", sessions
        FROM users
        WHERE username = $1
        "#,
        credentials.username
    )
    .fetch_one(&pool)
    .await?;

    Argon2::default()
        .verify_password(
            credentials.password.as_bytes(),
            &PasswordHash::new(user.hash.as_str())?,
        )
        .map_err(|e| match e {
            password_hash::Error::Password => PhsError::UNAUTHORIZED,
            _ => PhsError::INTERNAL,
        })?;

    // Credentials are correct as of here

    session.insert(super::SESSION_DATA_KEY, &user).await?;

    sqlx::query!(
        r#"UPDATE users SET sessions = array_append(sessions, $1) WHERE id = $2"#,
        id_type_to_hash(session.id().await).ok_or(PhsError::INTERNAL)?,
        user.id
    )
    .fetch_one(&pool)
    .await?;

    Ok("Logged in!")
}

fn id_type_to_hash(id: IdType) -> Option<String> {
    match id {
        IdType::Id(id) | IdType::Unloaded(id) => Some(hex::encode(Sha256::digest(id.to_string()))),
        _ => None,
    }
}

async fn whoami(session: AuthSession) -> Result<String, PhsError> {
    Ok(session.user.username)
}

async fn logout(
    mut auth_session: AuthSession,
    Extension(pool): Extension<PgPool>,
) -> Result<(), PhsError> {
    let user_id = auth_session.user.id;
    let session_id = auth_session.session.id().await;

    auth_session.destroy().await?;

    sqlx::query!(
        r#"UPDATE users SET sessions = array_remove(sessions, $1) WHERE id = $2"#,
        id_type_to_hash(session_id).ok_or(PhsError::INTERNAL)?,
        user_id
    )
    .fetch_one(&pool)
    .await?;

    Ok(())
}

#[derive(Deserialize)]
struct PaginationOptions {
    pub page: i32,
    pub page_size: i32,
}

async fn get_groups(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,

    pagination: Query<PaginationOptions>,
    Extension(pool): Extension<PgPool>,
) -> Result<Json<Vec<Group>>, PhsError> {
    let groups = sqlx::query_as!(
        Group,
        r#"
        SELECT id, group_name, permissions as "permissions: Vec<Permission>"
        FROM groups
        LIMIT LEAST(100, $1)
        OFFSET $2
        "#,
        pagination.page_size,
        (pagination.page * pagination.page_size) as i64
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(groups))
}

#[derive(Deserialize)]
pub struct CreateGroupBody {
    group_name: String,
    permissions: Vec<Permission>,
}

async fn create_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<CreateGroupBody>,
) -> Result<Json<Group>, PhsError> {
    let group = sqlx::query_as!(
        Group,
        r#"
        insert into groups(group_name, permissions)
        values ($1, $2)
        returning id, group_name, permissions as "permissions: _"
        "#,
        body.group_name,
        body.permissions as Vec<Permission>
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(group))
}

#[derive(Deserialize)]
pub struct PutGroupBody {
    group_name: String,
    permissions: Vec<Permission>,
}

async fn put_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,

    Extension(pool): Extension<PgPool>,
    Query(id): Query<i32>,
    Json(body): Json<PutGroupBody>,
) -> Result<Json<Group>, PhsError> {
    let group = sqlx::query_as!(
        Group,
        r#"
        update groups
        set group_name = $1, permissions = $2
        where id = $3
        returning id, group_name, permissions as "permissions: _"
        "#,
        body.group_name,
        body.permissions as Vec<Permission>,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(group))
}

async fn delete_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,

    Extension(pool): Extension<PgPool>,
    Query(id): Query<i32>,
) -> Result<(), PhsError> {
    sqlx::query!(r#"delete from groups where id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}

#[derive(Deserialize)]
struct ManageGroupParams {
    user: i32,
    group: i32,
}

async fn add_to_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    params: Query<ManageGroupParams>,
    Extension(pool): Extension<PgPool>,
) -> Result<(), PhsError> {
    sqlx::query!(
        r#"INSERT INTO users_groups(user_id, group_id) VALUES($1, $2)"#,
        params.user,
        params.group
    )
    .fetch_one(&pool)
    .await?;

    Ok(())
}

async fn delete_from_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as i32 }>,
    _: RequirePermission<{ Permission::ManageUsers as i32 }>,

    params: Query<ManageGroupParams>,
    Extension(pool): Extension<PgPool>,
) -> Result<(), PhsError> {
    sqlx::query!(
        r#"DELETE FROM users_groups WHERE user_id = $1 AND group_id = $2"#,
        params.user,
        params.group
    )
    .fetch_one(&pool)
    .await?;

    Ok(())
}
