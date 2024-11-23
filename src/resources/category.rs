use axum::{
    extract::Path,
    routing::{delete, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool};
use tracing::instrument;

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
            "/v1/categories/:id",
            delete(delete_tag).put(put_tag).get(get_tag),
        )
        .route("/v1/categories", post(create_tag).get(get_tags))
}

async fn get_tags(Extension(pool): Extension<PgPool>) -> Result<Json<Vec<Category>>, PhsError> {
    let tags = sqlx::query_as!(Category, "SELECT id, category FROM categories LIMIT 100")
        .fetch_all(&pool)
        .await?;

    Ok(Json(tags))
}

#[instrument(skip(pool))]
async fn get_tag(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(
        Category,
        r#"
        SELECT id,
            category
        FROM categories
        WHERE id = $1
        "#,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(tag))
}

#[derive(Debug, Deserialize)]
struct CreateCategoryBody {
    tag: String,
}

#[instrument(skip(pool, _auth_session))]
async fn create_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateCategoryBody>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(
        Category,
        r#"
        INSERT INTO categories(category)
        VALUES ($1)
        RETURNING id, category
        "#,
        req.tag
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(tag))
}

#[derive(Deserialize, Debug)]
struct PutTagBody {
    new: String,
}

#[instrument(skip(pool, _auth_session))]
async fn put_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(body): Json<PutTagBody>,
) -> Result<Json<Category>, PhsError> {
    let tag = sqlx::query_as!(
        Category,
        r#"
        UPDATE categories
        SET category = $1
        WHERE id = $2
        RETURNING id, category
        "#,
        body.new,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(tag))
}

#[instrument(skip(pool, _auth_session))]
async fn delete_tag(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditCategories as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query!(r#"DELETE FROM categories WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}
