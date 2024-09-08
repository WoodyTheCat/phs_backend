use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{Path, Query},
    routing::{post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;
use tera::Tera;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt, BufWriter},
    sync::Mutex,
};
use tracing::instrument;

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
    PaginationOptions,
};

use super::{render::Renderer, DynamicPageData, DynamicPageMetadata};

use slugify::slugify;

pub fn router() -> Router {
    Router::new()
        .route("/v1/pages", post(post_new_dynamic_page))
        .route("/v1/pages/:id", put(put_dynamic_page))
        .route("/v1/deploy", post(post_deploy_dynamic_pages))
}

#[derive(Deserialize, Debug)]
struct PostNewPage {
    name: String,
    data: DynamicPageData,
}

#[instrument(skip(pool, _auth_session))]
async fn post_new_dynamic_page(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<PostNewPage>,
) -> Result<(), PhsError> {
    let name = slugify::slugify!(&body.name, separator = "_");

    sqlx::query!(
        "INSERT INTO pages (name, modified) VALUES ($1, 'new'::page_status)",
        name
    )
    .execute(&pool)
    .await?;

    let mut writer = BufWriter::new(File::create_new(format!("pages/specs/{name}.json")).await?);

    writer
        .write_all(serde_json::ser::to_string(&body.data)?.as_bytes())
        .await?;

    writer.flush().await?;

    drop(writer);

    Renderer::render_fragment(
        PathBuf::from(format!("pages/fragments/{name}.html")),
        body.data,
    )
    .await?;

    Ok(())
}

#[instrument(skip(pool, _auth_session))]
async fn put_dynamic_page(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(data): Json<DynamicPageData>,
) -> Result<(), PhsError> {
    // TODO Slugify name

    let name = sqlx::query_scalar!(
        "UPDATE pages SET modified = 'edited'::page_status WHERE id = $1 RETURNING name",
        id
    )
    .fetch_one(&pool)
    .await?;

    let mut writer = BufWriter::new(
        File::options()
            .read(true)
            .write(true)
            .open(format!("pages/specs/{name}.json"))
            .await?,
    );

    writer
        .write_all(serde_json::ser::to_string(&data)?.as_bytes())
        .await?;

    writer.flush().await?;

    drop(writer);

    Renderer::render_fragment(PathBuf::from(format!("pages/fragments/{name}.html")), data).await?;

    Ok(())
}
#[instrument(skip(pool, _auth_session))]
async fn get_dynamic_page_metadata(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Query(pagination): Query<PaginationOptions>,
) -> Result<Json<Vec<DynamicPageMetadata>>, PhsError> {
    let pages = sqlx::query_as!(
        DynamicPageMetadata,
        r#"
            SELECT
            id, name, created_at, updated_at, modified as "modified: _"
            FROM pages
            ORDER BY name DESC
            LIMIT LEAST($1, 100)
            OFFSET $2
        "#,
        pagination.page_size,
        i64::from(pagination.page_size * pagination.page)
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(pages))
}

#[instrument(skip(pool, _auth_session))]
async fn post_deploy_dynamic_pages(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Extension(tera): Extension<Arc<Mutex<Tera>>>,
    Json(body): Json<Vec<i32>>,
) -> Result<(), PhsError> {
    let pages = sqlx::query_scalar!(
        //r#"SELECT name FROM pages WHERE id = ANY ($1) AND modified = ANY (ARRAY['new', 'edited']::page_status[])"#,
        r#"UPDATE pages SET modified = 'unmodified'::page_status WHERE id = ANY ($1) AND modified = ANY (ARRAY['new', 'edited']::page_status[]) RETURNING name"#,
        &body
    )
    .fetch_all(&pool)
    .await?;

    // FIXME: Only one endpoint can use the instance at a time...
    let tera = &mut tera.lock().await;

    tracing::debug!(?pages, "Pages to deploy");

    for page_name in pages {
        deploy_page(page_name, tera).await?;
    }

    Ok(())
}

async fn deploy_page(slug: String, tera: &mut Tera) -> Result<(), PhsError> {
    let mut fragment = String::new();
    let mut context = tera::Context::new();
    context.insert("title", &slug);

    {
        tokio::fs::File::open(format!("pages/fragments/{slug}.html"))
            .await?
            .read_to_string(&mut fragment)
            .await?;
    }

    let mut dist = tokio::fs::File::create(format!("pages/dist/{slug}.html")).await?;

    let str = tera.render_str(&fragment, &context).unwrap();

    dist.write_all(str.as_bytes()).await?;

    Ok(())
}
