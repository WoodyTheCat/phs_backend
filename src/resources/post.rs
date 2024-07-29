use axum::{
    extract::{Path, Query},
    routing::{delete, get},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, types::time::Date, PgPool};

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
};

pub fn router() -> Router {
    Router::new()
        .route("/v1/posts", get(get_posts).post(new_post))
        .route("/v1/post/:id", delete(delete_post).put(put_post))
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct Post {
    id: i32,

    title: String,
    content: String,

    author: Option<i32>,
    date: Date, // Defaults to creation date

    pinned: bool,
    department: Option<i32>,
    category: Option<i32>,
}

#[derive(Deserialize)]
struct PostSelectOptions {
    category: Option<i32>,
    department: Option<i32>,
    author: Option<i32>,
}

#[derive(Deserialize)]
struct PaginationOptions {
    page: i32,
    page_size: i32,
}

async fn get_posts(
    Extension(pool): Extension<PgPool>,
    select_options: Query<PostSelectOptions>,
    pagination: Query<PaginationOptions>,
) -> Result<Json<Vec<Post>>, PhsError> {
    let posts = sqlx::query_as!(
        Post,
        r#"
            SELECT * FROM posts
            WHERE ($1::integer IS NULL OR author = $1)
              AND ($2::integer IS NULL OR category = $2)
              AND ($3::integer IS NULL OR department = $3)
            ORDER BY pinned DESC, date DESC
            LIMIT LEAST(100, $4)
            OFFSET $5
            "#,
        select_options.author,
        select_options.category,
        select_options.department,
        pagination.page_size,
        (pagination.page * pagination.page_size) as i64
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(posts))
}

#[derive(Deserialize)]
struct NewPostBody {
    title: String,
    content: String,
    pinned: bool,
    department: Option<i32>,
    category: Option<i32>,
}

async fn new_post(
    auth_session: AuthSession,
    _: RequirePermission<{ Permission::CreatePosts as i32 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<NewPostBody>,
) -> Result<Json<Post>, PhsError> {
    let user = auth_session.user();

    let post = sqlx::query_as!(
        Post,
        r#"
            INSERT INTO posts (title, content, author, pinned, department, category)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        body.title,
        body.content,
        user.id,
        body.pinned,
        body.department,
        body.category,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(post))
}

// TODO: Auth(teacher), check author/admin
async fn delete_post(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditPosts as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query_as!(
        Post,
        r#"DELETE FROM posts WHERE id = $1"#, //  AND (author = $2 OR $2 = 0)
        id,
    )
    .execute(&pool)
    .await?;

    Ok(())
}

#[derive(Deserialize)]
struct PostPutBody {
    title: Option<String>,
    content: Option<String>,
    pinned: Option<bool>,
    department: Option<i32>,
    category: Option<i32>,
}

// TODO: Auth(teacher), check author/admin
async fn put_post(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditPosts as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    put_body: Json<PostPutBody>,
) -> Result<Json<Post>, PhsError> {
    let post = sqlx::query_as!(
        Post,
        r#"
            UPDATE posts
            SET title = $1,
                content = $2,
                pinned = $3,
                department = $4,
                category = $5
            WHERE id = $6
            RETURNING *
            "#, //  AND (author = $7 OR $7 = 0)
        put_body.title,
        put_body.content,
        put_body.pinned,
        put_body.department,
        put_body.category,
        id,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(post))
}
