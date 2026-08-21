use crate::error::{HybridSearchError, Result};
use crate::sources::{SpecialistPage, get_json};
use reqwest::Client;
use serde_json::Value;
use url::Url;

const STACKEXCHANGE_FILTER: &str = "!X-cWn5YrCQCchzB5B4*yqi6eO0BYWbSmsTE.VZm";

struct Answer {
    accepted: bool,
    score: i64,
    author: String,
    body: String,
}

pub(crate) fn matches(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    if !is_stackexchange_host(host) {
        return false;
    }
    let segments: Vec<&str> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect();
    segments.len() >= 2 && segments[0] == "questions" && segments[1].parse::<u64>().is_ok()
}

pub(crate) async fn fetch(
    client: &Client,
    url: &Url,
    max_answers: usize,
) -> Result<SpecialistPage> {
    let host = url.host_str().unwrap_or_default();
    let site = site_parameter(host);
    let question_id = url
        .path_segments()
        .into_iter()
        .flatten()
        .find(|segment| segment.parse::<u64>().is_ok())
        .ok_or_else(|| {
            HybridSearchError::Parse("StackExchange question ID is missing".to_string())
        })?;
    let question_url = api_url(
        &format!("questions/{question_id}"),
        &site,
        &[("filter", STACKEXCHANGE_FILTER)],
    );
    let pagesize = max_answers.saturating_add(1).min(100).to_string();
    let answer_url = api_url(
        &format!("questions/{question_id}/answers"),
        &site,
        &[
            ("filter", STACKEXCHANGE_FILTER),
            ("order", "desc"),
            ("sort", "votes"),
            ("pagesize", &pagesize),
        ],
    );

    let (question, answers) = tokio::join!(
        get_json(client, &question_url, "StackExchange question"),
        get_json(client, &answer_url, "StackExchange answers")
    );
    let question = question?
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| {
            HybridSearchError::Provider("StackExchange returned no question".to_string())
        })?;
    let mut answers = answers
        .map(|response| parse_answers(&response))
        .unwrap_or_default();
    answers.sort_by(|left, right| {
        right
            .accepted
            .cmp(&left.accepted)
            .then(right.score.cmp(&left.score))
    });

    Ok(SpecialistPage {
        content: render(&question, &answers, max_answers),
        source_type: "stackexchange",
    })
}

fn api_url(path: &str, site: &str, parameters: &[(&str, &str)]) -> String {
    let mut url = Url::parse(&format!("https://api.stackexchange.com/2.3/{path}"))
        .expect("StackExchange API URL must be valid");
    url.query_pairs_mut().append_pair("site", site);
    for (name, value) in parameters {
        url.query_pairs_mut().append_pair(name, value);
    }
    url.into()
}

fn site_parameter(host: &str) -> String {
    if let Some(base) = host.strip_prefix("meta.") {
        if base == "stackexchange.com" {
            return "meta.stackexchange".to_string();
        }
        return format!("meta.{}", base_site_parameter(base));
    }
    base_site_parameter(host)
}

fn base_site_parameter(host: &str) -> String {
    match host {
        "stackoverflow.com" => "stackoverflow".to_string(),
        "serverfault.com" => "serverfault".to_string(),
        "superuser.com" => "superuser".to_string(),
        "askubuntu.com" => "askubuntu".to_string(),
        "mathoverflow.net" => "mathoverflow.net".to_string(),
        other => other
            .strip_suffix(".stackexchange.com")
            .unwrap_or(other)
            .to_string(),
    }
}

fn is_stackexchange_host(host: &str) -> bool {
    matches!(
        host,
        "stackoverflow.com"
            | "serverfault.com"
            | "superuser.com"
            | "askubuntu.com"
            | "mathoverflow.net"
            | "meta.stackoverflow.com"
            | "meta.serverfault.com"
            | "meta.superuser.com"
            | "meta.askubuntu.com"
            | "meta.mathoverflow.net"
    ) || host.ends_with(".stackexchange.com")
}

fn parse_answers(response: &Value) -> Vec<Answer> {
    response
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|answer| Answer {
            accepted: answer
                .get("is_accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            score: answer
                .get("score")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            author: answer
                .get("owner")
                .and_then(|owner| owner.get("display_name"))
                .and_then(Value::as_str)
                .map(decode_entities)
                .unwrap_or_default(),
            body: markdown_body(answer),
        })
        .collect()
}

fn render(question: &Value, answers: &[Answer], max_answers: usize) -> String {
    let title = question
        .get("title")
        .and_then(Value::as_str)
        .map(decode_entities)
        .unwrap_or_default();
    let mut output = format!("# {title}\n\n{}\n\n---\n\n", markdown_body(question));
    for answer in answers.iter().take(max_answers) {
        if answer.accepted {
            output.push_str(&format!(
                "## Accepted answer (score: {})\n\n{}\n\n",
                answer.score, answer.body
            ));
        } else {
            output.push_str(&format!(
                "## Answer by {} (score: {})\n\n{}\n\n",
                answer.author, answer.score, answer.body
            ));
        }
    }
    if answers.len() > max_answers {
        output.push_str(&format!(
            "_{} additional answers were omitted._\n",
            answers.len() - max_answers
        ));
    }
    output
}

fn markdown_body(value: &Value) -> String {
    value
        .get("body_markdown")
        .or_else(|| value.get("body"))
        .and_then(Value::as_str)
        .map(decode_entities)
        .unwrap_or_default()
}

fn decode_entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(position) = remaining.find('&') {
        output.push_str(&remaining[..position]);
        let tail = &remaining[position..];
        let decoded = tail.find(';').and_then(|end| {
            let entity = &tail[1..end];
            let character = match entity {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity.strip_prefix('#').and_then(|number| {
                    match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => number.parse::<u32>().ok(),
                    }
                    .and_then(char::from_u32)
                }),
            };
            character.map(|character| (character, end))
        });
        if let Some((character, end)) = decoded {
            output.push(character);
            remaining = &tail[end + 1..];
        } else {
            output.push('&');
            remaining = &tail[1..];
        }
    }
    output.push_str(remaining);
    output
}
