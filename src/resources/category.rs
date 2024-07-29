use axum::{
    extract::Path,
    routing::{delete, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool};

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
};

#[derive(FromRow, Serialize, Deserialize)]
pub struct Category {
    id: i32,
    category: String,
}

pub fn router() -> Router {
    Router::new()
        .route(
            "/v1/category/:id",
            delete(delete_tag).put(put_tag).get(get_tag),
        )
        .route("/v1/categories", post(create_tag).get(get_tags))
}

#[derive(Deserialize)]
struct CreateCategoryBody {
    tag: String,
}

async fn get_tags(Extension(pool): Extension<PgPool>) -> Result<Json<Vec<Category>>, PhsError> {
    let tags = sqlx::query_as!(Category, "SELECT * FROM categories LIMIT 100")
        .fetch_all(&pool)
        .await?;

    Ok(Json(tags))
}

async fn get_tag(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(Category, r#"SELECT * FROM categories WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(Json(tag))
}

async fn create_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as i32 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateCategoryBody>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(
        Category,
        r#"INSERT INTO categories(category) VALUES ($1) RETURNING *"#,
        req.tag
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(tag))
}

#[derive(Deserialize)]
struct PutTagBody {
    new: String,
}

async fn put_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(body): Json<PutTagBody>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(
        Category,
        r#"UPDATE categories SET category = $1 WHERE id = $2 RETURNING *"#,
        body.new,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(tag))
}

async fn delete_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query!(r#"DELETE FROM categories WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}
