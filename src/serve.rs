use axum::Router;
use serde::{Deserialize, Serialize};
use time::PrimitiveDateTime;

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
    fn to_tag(self) -> (String, String) {
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
    fn to_tag(self) -> (String, String) {
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

#[derive(Serialize, Deserialize, Debug)]
pub struct DynamicPageMetadata {
    id: i32,
    name: String,

    created_at: PrimitiveDateTime,
    updated_at: PrimitiveDateTime,

    modified: PageStatus,
}

#[derive(Serialize, Deserialize, Debug, sqlx::Type)]
#[sqlx(type_name = "page_status", rename_all = "lowercase")]
pub enum PageStatus {
    Unmodified,
    New,
    Edited,
}
