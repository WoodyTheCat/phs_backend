use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{Path, Query},
    routing::{post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use sqlx::{prelude::FromRow, PgPool};
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
    resources::HasSqlxQueryString,
    serve::PageStatus,
    CursorOptions, CursorResponse,
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
    unsafe_name: String,
    data: DynamicPageData,
}

#[instrument(skip(pool, _auth_session))]
async fn post_new_dynamic_page(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<PostNewPage>,
) -> Result<(), PhsError> {
    let name = slugify::slugify!(&body.unsafe_name, separator = "_");

    sqlx::query!(
        "INSERT INTO pages (name, modified) VALUES ($1, 'new'::page_status)",
        name
    )
    .execute(&pool)
    .await?;

    let spec_path = {
        let mut p = PathBuf::from("pages/specs");
        p.push(&name);
        p.set_extension(".json");
        p
    };

    let temp_path = {
        let mut p = spec_path.clone();
        p.set_extension(".json.temp");
        p
    };

    // Tempfile for psuedo-atomic writes
    let mut writer = BufWriter::new(File::create_new(&temp_path).await?);

    writer
        .write_all(serde_json::ser::to_string(&body.data)?.as_bytes())
        .await?;

    writer.flush().await?;

    drop(writer);

    tokio::fs::rename(temp_path, spec_path).await?;

    let fragment_path = {
        let mut p = PathBuf::from("pages/fragments");
        p.push(&name);
        p.set_extension("html");
        p
    };

    Renderer::render_fragment(fragment_path, body.data).await?;

    Ok(())
}

// FIXME: Past me, please don't use format! so much... Also in the other endpoints in this file
#[instrument(skip(pool, _auth_session))]
async fn put_dynamic_page(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(data): Json<DynamicPageData>,
) -> Result<(), PhsError> {
    let name = sqlx::query_scalar!(
        "UPDATE pages SET modified = 'edited'::page_status WHERE id = $1 RETURNING name",
        id
    )
    .fetch_one(&pool)
    .await?;

    let spec_path = {
        let mut p = PathBuf::from("pages/specs");
        p.push(&name);
        p.set_extension(".json");
        p
    };
    let temp_path = {
        let mut p = spec_path.clone();
        p.set_extension(".json.temp");
        p
    };
    let fragment_path = {
        let mut p = PathBuf::from("pages/fragments");
        p.push(&name);
        p.set_extension("html");
        p
    };

    // Tempfile for psuedo-atomic writes
    let mut writer = BufWriter::new(File::options().write(true).open(&temp_path).await?);

    writer
        .write_all(serde_json::ser::to_string(&data)?.as_bytes())
        .await?;

    writer.flush().await?;

    drop(writer);

    tokio::fs::rename(temp_path, spec_path).await?;

    Renderer::render_fragment(fragment_path, data).await?;

    Ok(())
}
#[instrument(skip(pool, _auth_session))]
async fn get_dynamic_page_metadata(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::ManagePages as u8 }>,

    Query(cursor_options): Query<CursorOptions>,
    Query(query_string): Query<<DynamicPageMetadata as HasSqlxQueryString>::QueryString>,

    Extension(pool): Extension<PgPool>,
) -> Result<Json<CursorResponse<DynamicPageMetadata>>, PhsError> {
    let pages = crate::resources::paginated_query_as::<DynamicPageMetadata>(
        r"SELECT id, name, created_at, updated_at, modified FROM pages",
        cursor_options,
        query_string,
        &pool,
    )
    .await?;

    Ok(Json(CursorResponse::new(pages)))
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
        r#"UPDATE pages SET modified = 'unmodified'::page_status WHERE id = ANY ($1) AND modified = ANY (ARRAY['new', 'edited']::page_status[]) RETURNING name"#,
        &body
    )
    .fetch_all(&pool)
    .await?;

    tracing::debug!(?pages, "Pages to deploy");

    for page_name in pages {
        // FIXME: Only one endpoint can use the instance at a time...
        deploy_page(page_name, &mut *tera.lock().await).await?;
    }

    Ok(())
}

async fn deploy_page(slug: String, tera: &mut Tera) -> Result<(), PhsError> {
    let mut fragment = String::new();
    let mut context = tera::Context::new();
    context.insert("title", &slug);

    let fragment_path = {
        let mut p = PathBuf::from("pages/fragments");
        p.push(&slug);
        p.set_extension("html");
        p
    };

    {
        tokio::fs::File::open(fragment_path)
            .await?
            .read_to_string(&mut fragment)
            .await?;
    }

    let dist_path = {
        let mut p = PathBuf::from("pages/dist");
        p.push(&slug);
        p.set_extension(".html");
        p
    };

    let dist_temp_path = {
        let mut p = dist_path.clone();
        p.set_extension(".html.temp");
        p
    };

    // Tempfile for psuedo-atomic writes
    let mut dist = tokio::fs::File::create(&dist_temp_path).await?;

    let str = tera.render_str(&fragment, &context).unwrap();

    dist.write_all(str.as_bytes()).await?;

    tokio::fs::rename(dist_temp_path, dist_path).await?;

    Ok(())
}
