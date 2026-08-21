use crate::error::{HybridSearchError, Result};
use crate::sources::{SpecialistPage, get_json};
use percent_encoding::percent_decode_str;
use reqwest::Client;
use serde_json::Value;
use url::Url;

const EXCLUDED_NAMESPACES: &[&str] = &[
    "Special",
    "Talk",
    "Category",
    "Help",
    "User",
    "Wikipedia",
    "File",
    "Template",
    "Portal",
    "Draft",
];

pub(crate) fn matches(url: &Url) -> bool {
    let Some(language) = url
        .host_str()
        .and_then(|host| host.strip_suffix(".wikipedia.org"))
    else {
        return false;
    };
    if language.is_empty() || !url.path().starts_with("/wiki/") {
        return false;
    }
    let title = &url.path()["/wiki/".len()..];
    !title.is_empty() && !is_excluded_namespace(title)
}

pub(crate) async fn fetch(client: &Client, url: &Url) -> Result<SpecialistPage> {
    let language = url
        .host_str()
        .and_then(|host| host.strip_suffix(".wikipedia.org"))
        .ok_or_else(|| HybridSearchError::Parse("invalid Wikipedia host".to_string()))?;
    let title = percent_decode_str(&url.path()["/wiki/".len()..]).decode_utf8_lossy();
    let mut api_url = Url::parse(&format!("https://{language}.wikipedia.org/w/api.php"))
        .map_err(|error| HybridSearchError::Parse(format!("invalid Wikipedia API URL: {error}")))?;
    api_url
        .query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("prop", "extracts")
        .append_pair("explaintext", "true")
        .append_pair("titles", &title)
        .append_pair("format", "json")
        .append_pair("redirects", "1");

    let response = get_json(client, api_url.as_str(), "Wikipedia").await?;
    let page = first_page(&response)?;
    let title = page
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let extract = page
        .get("extract")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if extract.trim().is_empty() {
        return Err(HybridSearchError::Provider(
            "Wikipedia returned empty article content".to_string(),
        ));
    }

    Ok(SpecialistPage {
        content: format!("# {title}\n\n{extract}\n"),
        source_type: "wikipedia",
    })
}

fn is_excluded_namespace(title: &str) -> bool {
    let decoded = percent_decode_str(title).decode_utf8_lossy();
    let namespace = decoded.split(':').next().unwrap_or_default();
    EXCLUDED_NAMESPACES
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(namespace))
}

fn first_page(response: &Value) -> Result<&Value> {
    response
        .get("query")
        .and_then(|query| query.get("pages"))
        .and_then(Value::as_object)
        .and_then(|pages| pages.values().next())
        .ok_or_else(|| HybridSearchError::Provider("Wikipedia returned no page".to_string()))
}
