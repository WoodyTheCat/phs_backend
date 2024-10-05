use axum::{
    extract::{Path, Query},
    routing::{delete, get},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool, QueryBuilder};
use time::OffsetDateTime;
use tracing::instrument;

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
    CursorOptions, CursorPaginatable, CursorResponse,
};

use super::{HasSqlxQueryString, SqlxQueryString};

pub fn router() -> Router {
    Router::new()
        .route("/v1/posts", get(get_posts).post(new_post))
        .route(
            "/v1/post/:id",
            delete(delete_post).put(put_post).get(get_post),
        )
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct Post {
    id: i32,

    title: String,
    content: String,

    author: Option<i32>,
    #[serde(with = "time::serde::iso8601")]
    date: OffsetDateTime, // Defaults to creation date

    pinned: bool,
    department: Option<i32>,
    category: Option<i32>,
}

impl HasSqlxQueryString for Post {
    type QueryString = PostQueryString;
}

#[derive(Deserialize, Debug)]
pub struct PostQueryString {
    id: Option<i32>,
    title: Option<String>,
    author: Option<Option<i32>>,

    #[serde(with = "time::serde::iso8601::option", rename = "date[gte]")]
    date_gte: Option<OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option", rename = "date[lte]")]
    date_lte: Option<OffsetDateTime>,

    pinned: Option<bool>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    department: Option<Option<i32>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    category: Option<Option<i32>>,

    sort_by: Option<String>,
}

impl SqlxQueryString for PostQueryString {
    fn where_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) {
        if let Some(id) = self.id {
            builder.push(" AND id = ");
            builder.push_bind(id);
        }

        if let Some(title) = &self.title {
            builder.push(" AND title LIKE ");
            builder.push_bind(title);
        }

        if let Some(author) = &self.author {
            builder.push(" AND author = ");
            builder.push_bind(author);
        }

        if let Some(date_lte) = &self.date_lte {
            builder.push(" AND created <= ");
            builder.push_bind(date_lte);
        }

        if let Some(date_gte) = &self.date_gte {
            builder.push(" AND created >= ");
            builder.push_bind(date_gte);
        }

        if let Some(pinned) = self.pinned {
            builder.push(" AND pinned = ");
            builder.push_bind(pinned);
        }

        if let Some(department) = &self.department {
            builder.push(" AND department = ");
            builder.push_bind(department);
        }

        if let Some(category) = &self.category {
            builder.push(" AND category = ");
            builder.push_bind(category);
        }
    }

    fn order_by_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) -> bool {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            builder.push(", pinned DESC, date DESC");
            return false;
        };

        if let s @ ("id" | "title" | "content" | "author" | "date" | "pinned" | "department"
        | "category") = field.as_str()
        {
            builder.push(s);
            order.append_to(builder);
            true
        } else {
            false
        }
    }
}

impl CursorPaginatable for Post {
    fn id(&self) -> i32 {
        self.id
    }
}

#[instrument(skip(pool))]
async fn get_posts(
    Query(query_string): Query<<Post as HasSqlxQueryString>::QueryString>,
    Query(cursor_options): Query<CursorOptions>,

    Extension(pool): Extension<PgPool>,
) -> Result<Json<CursorResponse<Post>>, PhsError> {
    let posts: Vec<Post> = super::paginated_query_as::<Post>(
        r#"
        SELECT id,
          title,
          content,
          pinned,
          department,
          category,
          author,
          date as "date: _"
        FROM posts
        "#,
        cursor_options,
        query_string,
        &pool,
    )
    .await?;

    Ok(Json(CursorResponse::new(posts)))
}

#[instrument(skip(pool))]
async fn get_post(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Post>, PhsError> {
    let post = sqlx::query_as!(
        Post,
        r#"
        SELECT id,
            title,
            content,
            pinned,
            department,
            category,
            author,
            date as "date: _"
        FROM posts
        WHERE id = $1
        "#,
        id,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(post))
}

#[derive(Deserialize, Debug)]
struct NewPostBody {
    title: String,
    content: String,
    pinned: bool,
    department: Option<i32>,
    category: Option<i32>,
}

#[instrument(skip(pool, auth_session))]
async fn new_post(
    auth_session: AuthSession,
    _: RequirePermission<{ Permission::CreatePosts as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(body): Json<NewPostBody>,
) -> Result<Json<Post>, PhsError> {
    let user = auth_session.data();

    let post = sqlx::query_as!(
        Post,
        r#"
            INSERT INTO posts (
                title,
                content,
                author,
                pinned,
                department,
                category
            ) VALUES (
                $1, $2, $3, $4, $5, $6
            ) RETURNING id,
                title,
                content,
                pinned,
                department,
                category,
                author,
                date as "date: _"
            "#,
        body.title,
        body.content,
        user.id(),
        body.pinned,
        body.department,
        body.category,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(post))
}

#[instrument(skip(pool, _auth_session))]
async fn delete_post(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditPosts as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query_as!(Post, r#"DELETE FROM posts WHERE id = $1"#, id,)
        .execute(&pool)
        .await?;

    Ok(())
}

#[derive(Deserialize, Debug)]
struct PostPatchBody {
    title: String,
    content: String,
    author: Option<i32>,
    pinned: bool,
    department: Option<i32>,
    category: Option<i32>,
}

#[instrument(skip(pool, _auth_session))]
async fn put_post(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditPosts as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    put_body: Json<PostPatchBody>,
) -> Result<Json<Post>, PhsError> {
    let post = sqlx::query_as!(
        Post,
        r#"
            UPDATE posts
            SET title = $1,
                content = $2,
                pinned = $3,
                department = $4,
                category = $5,
                author = $6
            WHERE id = $7
            RETURNING id,
                title,
                content,
                pinned,
                department,
                category,
                author,
                date as "date: _"
            "#,
        put_body.title,
        put_body.content,
        put_body.pinned,
        put_body.department,
        put_body.category,
        put_body.author,
        id,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(post))
}
