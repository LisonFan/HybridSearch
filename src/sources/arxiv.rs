use crate::error::{HybridSearchError, Result};
use crate::sources::{SpecialistPage, get_text};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use reqwest::Client;
use url::Url;

struct ArxivPaper {
    title: String,
    authors: Vec<String>,
    categories: Vec<String>,
    summary: String,
    abstract_url: String,
    pdf_url: String,
}

pub(crate) fn matches(url: &Url) -> bool {
    if url.host_str() != Some("arxiv.org") {
        return false;
    }
    let path = url.path();
    (path.starts_with("/abs/") && path.len() > "/abs/".len())
        || (path.starts_with("/pdf/") && path.len() > "/pdf/".len())
}

pub(crate) async fn fetch(client: &Client, url: &Url) -> Result<SpecialistPage> {
    let id = extract_id(url)
        .ok_or_else(|| HybridSearchError::Parse("arXiv paper ID is missing".to_string()))?;
    let mut api_url =
        Url::parse("https://export.arxiv.org/api/query").expect("arXiv API URL must be valid");
    api_url.query_pairs_mut().append_pair("id_list", &id);
    let xml = get_text(client, api_url.as_str(), "arXiv").await?;
    let paper = parse_atom(&xml)?;

    Ok(SpecialistPage {
        content: render(&paper),
        source_type: "arxiv",
    })
}

fn extract_id(url: &Url) -> Option<String> {
    ["/abs/", "/pdf/"].into_iter().find_map(|prefix| {
        url.path().strip_prefix(prefix).and_then(|value| {
            (!value.is_empty()).then(|| value.strip_suffix(".pdf").unwrap_or(value).to_string())
        })
    })
}

fn parse_atom(xml: &str) -> Result<ArxivPaper> {
    #[derive(PartialEq)]
    enum Field {
        None,
        Title,
        Summary,
        Author,
    }

    let mut reader = Reader::from_str(xml);
    let mut in_entry = false;
    let mut in_author = false;
    let mut field = Field::None;
    let mut text = String::new();
    let mut title = String::new();
    let mut summary = String::new();
    let mut authors = Vec::new();
    let mut categories = Vec::new();
    let mut abstract_url = String::new();
    let mut pdf_url = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) => match element.name().as_ref() {
                b"entry" => in_entry = true,
                b"author" if in_entry => in_author = true,
                b"title" if in_entry => {
                    field = Field::Title;
                    text.clear();
                }
                b"summary" if in_entry => {
                    field = Field::Summary;
                    text.clear();
                }
                b"name" if in_author => {
                    field = Field::Author;
                    text.clear();
                }
                _ => {}
            },
            Ok(Event::Empty(element)) if in_entry => match element.name().as_ref() {
                b"category" => {
                    if let Some(term) = attribute(&element, b"term") {
                        categories.push(term);
                    }
                }
                b"link" => {
                    let href = attribute(&element, b"href").unwrap_or_default();
                    let content_type = attribute(&element, b"type").unwrap_or_default();
                    let relation = attribute(&element, b"rel").unwrap_or_default();
                    if content_type == "application/pdf" {
                        pdf_url = href;
                    } else if relation == "alternate" {
                        abstract_url = href;
                    }
                }
                _ => {}
            },
            Ok(Event::Text(value)) if field != Field::None => {
                let value = value.unescape().map_err(|error| {
                    HybridSearchError::Parse(format!("invalid arXiv XML text: {error}"))
                })?;
                text.push_str(value.as_ref());
            }
            Ok(Event::End(element)) => match element.name().as_ref() {
                b"title" if field == Field::Title => {
                    title = text.trim().to_string();
                    field = Field::None;
                }
                b"summary" if field == Field::Summary => {
                    summary = text.trim().to_string();
                    field = Field::None;
                }
                b"name" if field == Field::Author => {
                    authors.push(text.trim().to_string());
                    field = Field::None;
                }
                b"author" => in_author = false,
                b"entry" => {
                    in_entry = false;
                    in_author = false;
                    field = Field::None;
                }
                _ => {}
            },
            Err(error) => {
                return Err(HybridSearchError::Parse(format!(
                    "invalid arXiv XML: {error}"
                )));
            }
            _ => {}
        }
    }

    if title.is_empty() || summary.is_empty() {
        return Err(HybridSearchError::Parse(
            "arXiv response is missing a title or abstract".to_string(),
        ));
    }

    Ok(ArxivPaper {
        title,
        authors,
        categories,
        summary,
        abstract_url,
        pdf_url,
    })
}

fn attribute(element: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == key)
        .map(|attribute| String::from_utf8_lossy(&attribute.value).into_owned())
}

fn render(paper: &ArxivPaper) -> String {
    format!(
        "# {}\n\n**Authors:** {}\n\n**Categories:** {}\n\n**Links:** [Abstract]({}) | [PDF]({})\n\n## Abstract\n\n{}\n",
        paper.title,
        paper.authors.join(", "),
        paper.categories.join(", "),
        paper.abstract_url,
        paper.pdf_url,
        paper.summary,
    )
}
