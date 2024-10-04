use axum::Router;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use time::PrimitiveDateTime;

use crate::{
    resources::{HasSqlxQueryString, SqlxQueryString},
    CursorPaginatable,
};

mod page;
mod render;

pub fn router() -> Router {
    Router::new().merge(page::router())
}

pub type DynamicPageData = Vec<DynamicPageElement>;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum HeaderSize {
    H1,
    H2,
    H3,
    H4,
}

impl HeaderSize {
    fn into_tag(self) -> (String, String) {
        let (open, close) = match self {
            Self::H1 => ("<h1>", "</h1>"),
            Self::H2 => ("<h2>", "</h2>"),
            Self::H3 => ("<h3>", "</h3>"),
            Self::H4 => ("<h4>", "</h4>"),
        };

        (open.to_string(), close.to_string())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextModifier {
    Bold,
    Italic,
    Underline,
}

impl TextModifier {
    fn into_tag(self) -> (String, String) {
        let (open, close) = match self {
            Self::Bold => ("<b>", "</b>"),
            Self::Italic => ("<i>", "</i>"),
            Self::Underline => (r#"<p class="underline">"#, "</p>"),
        };

        (open.to_string(), close.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextComponent {
    modifiers: Vec<TextModifier>,
    link: Option<String>,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DynamicPageElement {
    Header {
        size: HeaderSize,
        contents: String,
    },
    Text {
        components: Vec<TextComponent>,
    },
    List {
        items: Vec<Vec<TextComponent>>,
        list_type: ListType,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ListType {
    Ordered,
    Unordered,
}

#[derive(Serialize, Deserialize, Debug, FromRow)]
pub struct DynamicPageMetadata {
    id: i32,
    name: String,

    created_at: PrimitiveDateTime,
    updated_at: PrimitiveDateTime,

    modified: PageStatus,
}

impl HasSqlxQueryString for DynamicPageMetadata {
    type QueryString = DynamicPageMetadataQueryString;
}

#[derive(Debug, Deserialize)]
pub struct DynamicPageMetadataQueryString {
    id: Option<i32>,
    name: Option<String>,
    modified: Option<PageStatus>,

    #[serde(rename = "created_at[gte]")]
    created_at_gte: Option<PrimitiveDateTime>,
    #[serde(rename = "created_at[lte]")]
    created_at_lte: Option<PrimitiveDateTime>,
    #[serde(rename = "updated_at[gte]")]
    updated_at_gte: Option<PrimitiveDateTime>,
    #[serde(rename = "updated_at[lte]")]
    updated_at_lte: Option<PrimitiveDateTime>,

    sort_by: Option<String>,
}

impl SqlxQueryString for DynamicPageMetadataQueryString {
    fn where_clause<'a>(&'a self, builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>) {
        if let Some(id) = self.id {
            builder.push(" AND id = ");
            builder.push_bind(id);
        }

        if let Some(ref name) = self.name {
            builder.push(" AND name LIKE ");
            builder.push_bind(name);
        }

        if let Some(ref modified) = self.modified {
            builder.push(" AND modified = ");
            builder.push_bind(modified);
        }

        if let Some(created_at_gte) = self.created_at_gte {
            builder.push(" AND created_at >= ");
            builder.push_bind(created_at_gte);
        }

        if let Some(created_at_lte) = self.created_at_lte {
            builder.push(" AND created_at <= ");
            builder.push_bind(created_at_lte);
        }

        if let Some(updated_at_gte) = self.updated_at_gte {
            builder.push(" AND updated_at >= ");
            builder.push_bind(updated_at_gte);
        }

        if let Some(updated_at_lte) = self.updated_at_lte {
            builder.push(" AND updated_at <= ");
            builder.push_bind(updated_at_lte);
        }
    }

    fn order_by_clause<'a>(&'a self, builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>) {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            return;
        };

        if let s @ ("id" | "name" | "modified" | "created_at" | "updated_at") = field.as_str() {
            builder.push(" ");
            builder.push(s);
            order.append_to(builder);
        }
    }
}

impl CursorPaginatable for DynamicPageMetadata {
    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Serialize, Deserialize, Debug, sqlx::Type, Clone, Copy)]
#[sqlx(type_name = "page_status", rename_all = "lowercase")]
pub enum PageStatus {
    Unmodified,
    New,
    Edited,
}
