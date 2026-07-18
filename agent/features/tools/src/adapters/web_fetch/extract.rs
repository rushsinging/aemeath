use scraper::{Html, Selector};
use std::collections::HashSet;

pub struct ExtractOptions<'a> {
    pub base_url: &'a str,
    pub max_content_bytes: usize,
    pub max_links: usize,
}

pub struct ExtractedPage {
    pub title: String,
    pub markdown: String,
    pub links: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("HTML content exceeds maximum allowed size")]
    TooLarge,
    #[error("failed to parse base URL: {0}")]
    BaseUrl(#[from] url::ParseError),
}

pub fn extract_page(html: &str, opts: ExtractOptions<'_>) -> Result<ExtractedPage, ExtractError> {
    if html.len() > opts.max_content_bytes {
        return Err(ExtractError::TooLarge);
    }

    let trimmed = html.trim_start();
    if !trimmed.starts_with('<') {
        return Ok(ExtractedPage {
            title: String::new(),
            markdown: html.to_string(),
            links: Vec::new(),
        });
    }

    let document = Html::parse_document(html);
    let title = extract_title(&document);
    let links = extract_links(&document, opts.base_url, opts.max_links)?;
    let markdown = extract_content_markdown(&document)?;

    Ok(ExtractedPage {
        title,
        markdown,
        links,
    })
}

fn extract_title(document: &Html) -> String {
    let selector = Selector::parse("title").unwrap();
    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

fn extract_links(
    document: &Html,
    base_url: &str,
    max_links: usize,
) -> Result<Vec<String>, ExtractError> {
    let base = url::Url::parse(base_url)?;
    let selector = Selector::parse("a[href]").unwrap();
    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for element in document.select(&selector) {
        if links.len() >= max_links {
            break;
        }
        if let Some(href) = element.value().attr("href") {
            if href.starts_with('#')
                || href.starts_with("javascript:")
                || href.starts_with("mailto:")
                || href.starts_with("tel:")
            {
                continue;
            }
            if let Ok(abs) = base.join(href) {
                let s = abs.to_string();
                if seen.insert(s.clone()) {
                    links.push(s);
                }
            }
        }
    }

    Ok(links)
}

fn extract_content_markdown(document: &Html) -> Result<String, ExtractError> {
    let selectors = [
        Selector::parse("main").unwrap(),
        Selector::parse("article").unwrap(),
        Selector::parse("body").unwrap(),
    ];

    let html_fragment = selectors
        .iter()
        .find_map(|sel| document.select(sel).next())
        .map(|el| el.html())
        .unwrap_or_else(|| document.html());

    let converter = htmd::HtmlToMarkdown::new();
    Ok(converter.convert(&html_fragment).unwrap_or(html_fragment))
}
