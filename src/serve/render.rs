use std::path::PathBuf;

use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};

use crate::{error::PhsError, serve::TextModifier};

use super::{DynamicPageElement, HeaderSize, ListType, TextComponent};

impl DynamicPageElement {
    pub fn render(self) -> String {
        match self {
            Self::Header { size, contents } => Self::render_header(size, &contents),
            Self::Text { components } => Self::render_text(components, ("<p>", "</p>")),
            Self::List { items, list_type } => Self::render_list(items, list_type),
        }
    }

    fn render_text(components: Vec<TextComponent>, wrappers: (&str, &str)) -> String {
        components
            .into_iter()
            .map(Self::render_text_component)
            .fold(String::from(wrappers.0), |acc, i| acc + &i)
            + wrappers.1
    }

    fn render_text_component(component: TextComponent) -> String {
        let (opening, mut closing): (Vec<_>, Vec<_>) = component
            .modifiers
            .into_iter()
            .map(TextModifier::to_tag)
            .unzip();

        closing.reverse();

        let content = if let Some(link) = component.link {
            format!(r#"<a href="{}">{}</a>"#, link, component.content)
        } else {
            component.content
        };

        format!("{}{}{}", opening.concat(), content, closing.concat())
    }

    fn render_list(items: Vec<Vec<TextComponent>>, list_type: ListType) -> String {
        let wrappers = match list_type {
            ListType::Unordered => ("<ul>", "</ul>"),
            ListType::Ordered => ("<ol>", "</ol>"),
        };

        items
            .into_iter()
            .map(|components| Self::render_text(components, ("<li>", "</li>")))
            .fold(String::from(wrappers.0), |acc, i| acc + &i)
            + wrappers.1
    }

    fn render_header(size: HeaderSize, contents: &str) -> String {
        let (opening, closing) = size.to_tag();

        format!("{opening}{contents}{closing}")
    }
}

pub struct Renderer;

static FRAGMENT_HEADER: &str = r#"
{% extends "base.html" %}

{% block title %}{{ title }}{% endblock main %}

{% block main %}
"#;

static FRAGMENT_FOOTER: &str = r"
{% endblock main %}
";

impl Renderer {
    pub async fn render_fragment(
        path: PathBuf,
        elements: Vec<DynamicPageElement>,
    ) -> Result<(), PhsError> {
        let file = File::create(path).await?;
        let mut writer = BufWriter::new(file);

        writer.write_all(FRAGMENT_HEADER.as_bytes()).await?;

        for html in elements.into_iter().map(DynamicPageElement::render) {
            writer.write_all(html.as_bytes()).await?;
        }

        writer.write_all(FRAGMENT_FOOTER.as_bytes()).await?;

        writer.flush().await?;

        Ok(())
    }
}
