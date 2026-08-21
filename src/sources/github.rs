use crate::error::{HybridSearchError, Result};
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub struct GithubPage {
    pub content: String,
    pub source_type: &'static str,
}

#[derive(Clone)]
struct Comment {
    author: String,
    body: String,
    created_at: String,
}

pub async fn fetch(
    client: &Client,
    url: &Url,
    token: Option<&str>,
    max_comments: usize,
) -> Result<Option<GithubPage>> {
    if url.host_str() != Some("github.com") {
        return Ok(None);
    }
    let segments: Vec<&str> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect();

    if segments.len() == 4
        && matches!(segments[2], "issues" | "pull")
        && segments[3].parse::<u64>().is_ok()
    {
        let is_pull = segments[2] == "pull";
        let content = fetch_issue_or_pull(
            client,
            segments[0],
            segments[1],
            segments[3],
            token,
            is_pull,
            max_comments,
        )
        .await?;
        return Ok(Some(GithubPage {
            content,
            source_type: if is_pull {
                "github_pull"
            } else {
                "github_issue"
            },
        }));
    }

    let is_latest_release =
        segments.len() == 4 && segments[2] == "releases" && segments[3] == "latest";
    let is_tag_release = segments.len() == 5 && segments[2] == "releases" && segments[3] == "tag";
    if is_latest_release || is_tag_release {
        let endpoint = if is_tag_release {
            format!(
                "https://api.github.com/repos/{}/{}/releases/tags/{}",
                segments[0], segments[1], segments[4]
            )
        } else {
            format!(
                "https://api.github.com/repos/{}/{}/releases/latest",
                segments[0], segments[1]
            )
        };
        let response = get_json(client, &endpoint, token, "GitHub release").await?;
        return Ok(Some(GithubPage {
            content: render_release(&response)?,
            source_type: "github_release",
        }));
    }

    Ok(None)
}

async fn fetch_issue_or_pull(
    client: &Client,
    owner: &str,
    repo: &str,
    number: &str,
    token: Option<&str>,
    is_pull: bool,
    max_comments: usize,
) -> Result<String> {
    let main_endpoint = if is_pull {
        format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}")
    } else {
        format!("https://api.github.com/repos/{owner}/{repo}/issues/{number}")
    };
    let per_page = max_comments.saturating_add(1).min(100);
    let conversation_endpoint = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues/{number}/comments?per_page={per_page}"
    );

    let (main, conversation) = tokio::join!(
        get_json(client, &main_endpoint, token, "GitHub issue"),
        get_json(client, &conversation_endpoint, token, "GitHub comments")
    );
    let main = main?;
    let mut comments = parse_comments(&conversation?);

    if is_pull {
        let review_comments_endpoint = format!(
            "https://api.github.com/repos/{owner}/{repo}/pulls/{number}/comments?per_page={per_page}"
        );
        let reviews_endpoint = format!(
            "https://api.github.com/repos/{owner}/{repo}/pulls/{number}/reviews?per_page={per_page}"
        );
        let (review_comments, reviews) = tokio::join!(
            get_json(
                client,
                &review_comments_endpoint,
                token,
                "GitHub review comments"
            ),
            get_json(client, &reviews_endpoint, token, "GitHub reviews")
        );
        if let Ok(value) = review_comments {
            comments.extend(parse_comments(&value));
        }
        if let Ok(value) = reviews {
            comments.extend(parse_reviews(&value));
        }
        comments.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    }

    Ok(render_issue_or_pull(
        &main,
        &comments,
        is_pull,
        max_comments,
    ))
}

async fn get_json(
    client: &Client,
    endpoint: &str,
    token: Option<&str>,
    label: &str,
) -> Result<Value> {
    let mut request = client
        .get(endpoint)
        .header(
            reqwest::header::USER_AGENT,
            "HybridSearch/0.1 (https://github.com/LisonFan/HybridSearch)",
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            HybridSearchError::Timeout(format!("{label} request"))
        } else {
            HybridSearchError::Provider(format!("{label} request failed: {error}"))
        }
    })?;
    let status = response.status();
    let body = response.bytes().await.map_err(|error| {
        HybridSearchError::Provider(format!("{label} response read failed: {error}"))
    })?;
    if !status.is_success() {
        return Err(HybridSearchError::Provider(format!(
            "{label} returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|error| HybridSearchError::Parse(format!("invalid {label} JSON: {error}")))
}

fn parse_comments(value: &Value) -> Vec<Comment> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| Comment {
            author: nested_string(item, "user", "login"),
            body: string(item, "body"),
            created_at: string(item, "created_at"),
        })
        .collect()
}

fn parse_reviews(value: &Value) -> Vec<Comment> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| Comment {
            author: nested_string(item, "user", "login"),
            body: string(item, "body"),
            created_at: string(item, "submitted_at"),
        })
        .filter(|comment| !comment.body.trim().is_empty())
        .collect()
}

fn render_issue_or_pull(
    value: &Value,
    comments: &[Comment],
    is_pull: bool,
    max_comments: usize,
) -> String {
    let title = string(value, "title");
    let state = string(value, "state");
    let author = nested_string(value, "user", "login");
    let body = string(value, "body");
    let labels = value
        .get("labels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|label| label.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let state_suffix = if is_pull && value.get("merged").and_then(Value::as_bool) == Some(true) {
        " (merged)"
    } else if is_pull && state == "closed" {
        " (closed, not merged)"
    } else {
        ""
    };

    let mut output =
        format!("# {title}\n\n**State:** {state}{state_suffix}\n**Author:** {author}\n");
    if !labels.is_empty() {
        output.push_str(&format!("**Labels:** {}\n", labels.join(", ")));
    }
    output.push_str(&format!("\n{body}\n\n## Comments\n\n"));
    for comment in comments.iter().take(max_comments) {
        output.push_str(&format!(
            "### Comment by {} ({})\n\n{}\n\n",
            comment.author, comment.created_at, comment.body
        ));
    }
    if comments.len() > max_comments {
        output.push_str(&format!(
            "_{} additional comments were omitted._\n",
            comments.len() - max_comments
        ));
    }
    output
}

fn render_release(value: &Value) -> Result<String> {
    let tag = string(value, "tag_name");
    if tag.is_empty() {
        return Err(HybridSearchError::Parse(
            "GitHub release response is missing tag_name".to_string(),
        ));
    }
    let name = string(value, "name");
    let title = if name.trim().is_empty() { &tag } else { &name };
    let prerelease = if value.get("prerelease").and_then(Value::as_bool) == Some(true) {
        " (prerelease)"
    } else {
        ""
    };
    Ok(format!(
        "# {title}\n\n**Tag:** {tag}{prerelease}\n**Published:** {}\n**Author:** {}\n\n{}\n",
        string(value, "published_at"),
        nested_string(value, "author", "login"),
        string(value, "body")
    ))
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn nested_string(value: &Value, object: &str, key: &str) -> String {
    value
        .get(object)
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
