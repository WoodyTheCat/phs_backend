use std::fmt::Debug;

use axum::Router;

mod category;
mod department;
mod post;
mod user;

pub use department::Department;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, FromRow, PgPool, QueryBuilder};
pub use user::Role;

use crate::error::PhsError;

pub fn router() -> Router {
    Router::new()
        .merge(user::router())
        .merge(post::router())
        .merge(category::router())
        .merge(department::router())
}

#[derive(Deserialize, Debug, Serialize)]
pub struct CursorOptions {
    #[serde(default)]
    cursor: i32,
    #[serde(default = "_default_cursor_length")]
    #[serde(rename = "cursor[length]")]
    length: i32,
    #[serde(default)]
    #[serde(rename = "cursor[previous]")]
    previous: bool,
}

#[rustfmt::skip]
const fn _default_cursor_length() -> i32 { 20 }

#[derive(Deserialize, Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CursorResponse<T> {
    next_cursor: Option<i32>,
    previous_cursor: i32,
    has_next_page: bool,

    data: Vec<T>,
}

pub trait CursorPaginatable {
    fn id(&self) -> i32;
}

impl<T: CursorPaginatable> CursorResponse<T> {
    pub fn new(data: Vec<T>) -> Self {
        let (previous_cursor, next_cursor) = (
            data.first().map_or(0, |v| <T as CursorPaginatable>::id(v)),
            data.last().map(|v| <T as CursorPaginatable>::id(v)),
        );

        Self {
            next_cursor,
            previous_cursor,
            // FIXME: ??? This doesn't do what it says...
            has_next_page: data.len() != 0,

            data,
        }
    }
}

pub enum SortOrder {
    Asc,
    Desc,
}

impl From<&str> for SortOrder {
    fn from(v: &str) -> Self {
        match v.to_lowercase().as_str() {
            "desc" => Self::Desc,
            _ => Self::Asc,
        }
    }
}

impl SortOrder {
    pub fn append_to(&self, builder: &mut QueryBuilder<sqlx::Postgres>) {
        builder.push(match self {
            Self::Desc => " DESC",
            Self::Asc => " ASC",
        });
    }
}

pub trait SqlxQueryString {
    fn where_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>);
    fn order_by_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) -> bool;

    fn parse_sort_by(sort_by: &Option<String>) -> Option<(String, SortOrder)> {
        sort_by.as_ref().map(|sb| {
            sb.split_once('.')
                .map(|(f, o)| (f.to_owned(), SortOrder::from(o)))
                .unwrap_or((sb.clone(), SortOrder::Asc))
        })
    }
}

pub trait HasSqlxQueryString {
    type QueryString: SqlxQueryString + Deserialize<'static> + Debug + Send;
}

pub async fn paginated_query_as<O>(
    init: &str,
    mut cursor: CursorOptions,
    query_string: <O as HasSqlxQueryString>::QueryString,
    pool: &PgPool,
) -> Result<Vec<O>, PhsError>
where
    O: HasSqlxQueryString + Send + Unpin + for<'r> FromRow<'r, PgRow>,
    Result<Vec<O>, PhsError>: Send,
{
    cursor.length = cursor.length.clamp(1, 200);

    let mut query_builder = QueryBuilder::new(init);

    query_builder.push(" WHERE id ");
    query_builder.push(if cursor.previous { "< " } else { "> " });
    query_builder.push_bind(cursor.cursor);
    query_string.where_clause(&mut query_builder);

    query_builder.push(" ORDER BY ");
    if query_string.order_by_clause(&mut query_builder) {
        query_builder.push(", ");
    }
    query_builder.push("id ASC");

    query_builder.push(" LIMIT ").push_bind(cursor.length);

    query_builder
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(Into::into)
}
