use super::Session;
use argon2::{password_hash, Argon2, PasswordHash, PasswordVerifier};
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::{
    auth::{AuthUser, Permission, UserPermissions},
    error::PhsError,
    resources::{CursorOptions, CursorResponse, HasSqlxQueryString, Role},
};

use super::{AuthSession, Group, RequirePermission};

pub fn router() -> Router {
    Router::new()
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", get(logout))
        .route("/v1/auth/whoami", get(whoami))
        .route("/v1/auth/groups", get(get_groups).post(create_group))
        .route("/v1/auth/group/:id", put(put_group).delete(delete_group))
        .route(
            "/v1/auth/users/groups",
            get(add_to_group).delete(delete_from_group),
        )
        .route("/v1/auth/users/permissions/:id", get(get_user_permissions))
        .route("/v1/auth/users/permissions", get(get_users_permissions))
}

#[derive(Deserialize)]
struct PostLoginBody {
    username: String,
    password: String,
}

async fn login(
    session: Session,
    Extension(pool): Extension<PgPool>,
    Json(credentials): Json<PostLoginBody>,
) -> Result<String, PhsError> {
    // TODO: Consolodate queries and remove UserWithHash type
    struct UserWithHash {
        id: i32,
        username: String,
        role: Role,
        hash: String,
        permissions: Vec<Permission>,
    }

    let user = sqlx::query_as!(
        UserWithHash,
        r#"
        SELECT id, username, role as "role: _", hash, permissions as "permissions: _"
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
            password_hash::Error::Password => {
                PhsError(StatusCode::UNAUTHORIZED, Some(Box::new(e)), "Unauthorised")
            }
            e => e.into(),
        })?;

    // Credentials are correct as of here

    // Get the user's groups and permissions
    let group_data = sqlx::query_as!(
        Group,
        r#"
        SELECT id, group_name, permissions as "permissions: _"
        FROM users_groups
        INNER JOIN groups
        ON groups.id = users_groups.group_id
        WHERE user_id = $1
        "#,
        user.id
    )
    .fetch_all(&pool)
    .await?;

    let mut permissions = group_data
        .iter()
        .flat_map(|gd| gd.permissions.iter())
        .copied()
        .collect::<Vec<Permission>>();

    // Add the user's override permissions to the vector
    permissions.extend(&user.permissions);

    let groups = group_data.into_iter().map(|gd| gd.group_name).collect();

    let auth_user = AuthUser {
        id: user.id,
        hash: user.hash.clone(),
        username: user.username.clone(),
        role: user.role,
        permissions,
        groups,
    };

    session.set(auth_user).await?;

    // Explicitly save the session so the ID is populated
    session.save().await?;

    // Then cycle the ID to prevent session fixation
    session.cycle_id().await?;

    let hashed_id = session.get_hashed_id().await.ok_or(PhsError(
        StatusCode::INTERNAL_SERVER_ERROR,
        None,
        "Error getting hashed session ID",
    ))?;

    tracing::info!({ user = ?user.id, hashed_id }, "Successful login");

    Ok("Logged in".into())
}

async fn whoami(session: AuthSession) -> Result<Json<i32>, PhsError> {
    Ok(Json(session.auth_user.id))
}

/// Logout only the current session
async fn logout(mut auth_session: AuthSession) -> Result<(), PhsError> {
    auth_session.destroy().await
}

async fn get_groups(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Query(cursor_options): Query<CursorOptions>,
    Query(query_string): Query<<Group as HasSqlxQueryString>::QueryString>,

    Extension(pool): Extension<PgPool>,
) -> Result<Json<CursorResponse<Group>>, PhsError> {
    crate::resources::paginated_query_as::<Group>(
        r"SELECT id, group_name, permissions FROM groups",
        cursor_options,
        query_string,
        &pool,
    )
    .await
    .map(|groups| Json(CursorResponse::new(groups)))
    .map_err(Into::into)
}

#[derive(Deserialize)]
pub struct CreateGroupBody {
    group_name: String,
    permissions: Vec<Permission>,
}

async fn create_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<CreateGroupBody>,
) -> Result<Json<Group>, PhsError> {
    sqlx::query_as!(
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
    .await
    .map(Json)
    .map_err(Into::into)
}

#[derive(Deserialize)]
pub struct PutGroupBody {
    group_name: String,
    permissions: Vec<Permission>,
}

async fn put_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(body): Json<PutGroupBody>,
) -> Result<Json<Group>, PhsError> {
    sqlx::query_as!(
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
    .await
    .map(Json)
    .map_err(Into::into)
}

async fn delete_group(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
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
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

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
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

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

async fn get_users_permissions(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Extension(pool): Extension<PgPool>,

    Query(cursor_options): Query<CursorOptions>,
    Query(query_string): Query<<UserPermissions as HasSqlxQueryString>::QueryString>,
) -> Result<Json<CursorResponse<UserPermissions>>, PhsError> {
    crate::resources::paginated_query_as::<UserPermissions>(
        r#"
        SELECT
            id, username, name,
            COALESCE(G.group_ids, array[]::int[]) AS group_ids,
            COALESCE(G.permissions, array[]::permission[]) AS permissions
        FROM
            users
        LEFT JOIN (
            SELECT
    	          UG.user_id AS id,
    	          ARRAY_AGG(DISTINCT G.id) AS group_ids,
    	          ARRAY_AGG(element) AS permissions
            FROM
                users_groups UG
            JOIN groups G ON G.id = UG.group_id,
            (
                SELECT
    	              UNNEST(permissions) AS element
                FROM
        	          groups
            )
            GROUP BY UG.user_id
        ) G
        USING (id)
        "#,
        cursor_options,
        query_string,
        &pool,
    )
    .await
    .map(|users_perms| Json(CursorResponse::new(users_perms)))
}

async fn get_user_permissions(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePermissions as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<UserPermissions>, PhsError> {
    sqlx::query_as!(
        UserPermissions,
        r#"
        SELECT
            id, username, name,
            COALESCE(G.group_ids, array[]::int[]) AS "group_ids!: _",
            COALESCE(G.permissions, array[]::permission[]) AS "permissions!: _"
        FROM
            users
        LEFT JOIN (
            SELECT
    	          UG.user_id AS id,
    	          ARRAY_AGG(DISTINCT G.id) AS group_ids,
    	          ARRAY_AGG(perms_set) AS permissions
            FROM
                users_groups UG
            JOIN groups G ON G.id = UG.group_id,
            (
                SELECT
    	              UNNEST(permissions) AS perms_set
                FROM
        	          groups
            )
            WHERE UG.user_id = $1
            GROUP BY UG.user_id
        ) G
        USING (id)
        "#,
        id
    )
    .fetch_one(&pool)
    .await
    .map(Json)
    .map_err(Into::into)
}
