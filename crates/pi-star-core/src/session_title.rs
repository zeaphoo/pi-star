//! One-shot session title generation, ported from pi-web's `lib/session-title.ts`.

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::agent::provider::{self, ProviderBlock, ProviderDelta, ProviderMessage};
use crate::models::ModelDef;

const TITLE_SYSTEM_PROMPT: &str =
    "You name chat sessions from a transcript. Reply with the title only.";
const TITLE_PROMPT: &str =
    "Create a concise title for this session based on the conversation above.

Requirements:
- Match the primary language used by the user.
- Describe the user's concrete goal or the outcome, not the act of chatting.
- Use 4-12 words for space-separated languages, or 8-24 characters for CJK text when practical.
- Do not call any tools.
- Return only the title as plain text, with no quotes, label, markdown, or explanation.";
const MAX_TITLE_LENGTH: usize = 80;
const TRANSCRIPT_CHARS: usize = 6000;

pub struct GeneratedTitle {
    pub title: String,
    pub usage: Option<Value>,
}

fn clip(text: &str, max: usize) -> String {
    let mut characters = text.chars();
    let clipped: String = characters.by_ref().take(max).collect();
    if characters.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

fn text_of(message: &Value) -> String {
    let content = message.get("content").unwrap_or(message);
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    let Some(blocks) = content.as_array() else {
        return String::new();
    };
    let mut parts = Vec::new();
    let mut images = 0;
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            Some("image") => images += 1,
            _ => {}
        }
    }
    let mut text = parts.join("\n").trim().to_string();
    if images > 0 {
        let marker = if images == 1 {
            "[image]".to_string()
        } else {
            format!("[image] ×{images}")
        };
        text = if text.is_empty() {
            marker
        } else {
            format!("{text}\n{marker}")
        };
    }
    text
}

/// Flatten a session into the transcript the title model reads.
pub fn build_transcript(entries: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut last_assistant = None;
    for entry in entries {
        let role = entry.pointer("/message/role").and_then(Value::as_str);
        if role == Some("assistant")
            && !text_of(entry.get("message").unwrap_or(entry))
                .trim()
                .is_empty()
        {
            last_assistant = Some(entry.get("message").unwrap_or(entry).clone());
        }
    }
    for entry in entries {
        if let Some(summary) = entry.get("summary").and_then(Value::as_str) {
            if !summary.trim().is_empty() {
                lines.push(format!("[Earlier summary] {}", clip(summary.trim(), 600)));
                continue;
            }
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "user" => {
                let text = text_of(message).trim().to_string();
                if !text.is_empty() {
                    lines.push(format!("User: {}", clip(&text, 800)));
                }
            }
            "assistant" => {
                let text = text_of(message).trim().to_string();
                if !text.is_empty() {
                    let is_last = last_assistant.as_ref().is_some_and(|last| last == message);
                    lines.push(format!(
                        "Assistant: {}",
                        clip(&text, if is_last { 600 } else { 300 })
                    ));
                }
            }
            "custom" => {
                let text = text_of(message).trim().to_string();
                if !text.is_empty() {
                    lines.push(format!("User: {}", clip(&text, 800)));
                }
            }
            "bashExecution" => {
                if let Some(command) = message.get("command").and_then(Value::as_str) {
                    if !command.trim().is_empty() {
                        lines.push(format!(
                            "User: {}",
                            clip(&format!("Ran `{}`", command.trim()), 800)
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    let joined = lines.join("\n\n");
    if joined.chars().count() <= TRANSCRIPT_CHARS || lines.len() < 2 {
        return joined;
    }
    let head_budget = TRANSCRIPT_CHARS * 2 / 5;
    let mut head = Vec::new();
    let mut used = 8;
    let mut next = 0;
    for line in &lines {
        let size = used + line.chars().count() + 2;
        if !head.is_empty() && size > head_budget {
            break;
        }
        head.push(line.clone());
        used = size;
        next += 1;
    }
    let mut tail = Vec::new();
    for line in lines[next..].iter().rev() {
        let size = used + line.chars().count() + 2;
        if size > TRANSCRIPT_CHARS {
            break;
        }
        tail.insert(0, line.clone());
        used = size;
    }
    [head, vec!["[…]".to_string()], tail].concat().join("\n\n")
}

fn strip_wrapping_quotes(value: &str) -> String {
    for (start, end) in [
        ('"', '"'),
        ('\'', '\''),
        ('`', '`'),
        ('\u{201c}', '\u{201d}'),
        ('\u{300c}', '\u{300d}'),
        ('\u{300e}', '\u{300f}'),
    ] {
        if value.starts_with(start) && value.ends_with(end) && value.chars().count() > 2 {
            return value[start.len_utf8()..value.len() - end.len_utf8()]
                .trim()
                .to_string();
        }
    }
    value.to_string()
}

pub fn parse_title(raw: &str) -> Result<String> {
    let mut value = raw.trim().to_string();
    if let Some(fenced) = value
        .strip_prefix("```")
        .and_then(|rest| rest.split_once('\n'))
        .and_then(|(_, rest)| rest.strip_suffix("```"))
    {
        value = fenced.trim().to_string();
    }
    if value.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<Value>(&value) {
            if let Some(title) = parsed.get("title").and_then(Value::as_str) {
                value = title.trim().to_string();
            }
        }
    }
    value = value.lines().next().unwrap_or("").to_string();
    let lower = value.to_ascii_lowercase();
    for prefix in ["session title", "title", "标题"] {
        if lower.starts_with(prefix) {
            let rest = &value[prefix.len()..];
            value = rest
                .trim_start_matches(|ch: char| {
                    ch == ':' || ch == '：' || ch == '-' || ch.is_whitespace()
                })
                .to_string();
            break;
        }
    }
    value = strip_wrapping_quotes(&value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    value = value.trim_end_matches(['.', '!', '。']).trim().to_string();
    if !value.chars().any(char::is_alphanumeric) {
        return Err(anyhow!("The model did not return a usable session title"));
    }
    Ok(value
        .chars()
        .take(MAX_TITLE_LENGTH)
        .collect::<String>()
        .trim()
        .to_string())
}

pub async fn generate_title(
    client: reqwest::Client,
    model: ModelDef,
    entries: &[Value],
) -> Result<GeneratedTitle> {
    let transcript = build_transcript(entries);
    if transcript.trim().is_empty() {
        return Err(anyhow!("The session has no usable text to name"));
    }
    let message = ProviderMessage {
        role: "user".to_string(),
        content: vec![ProviderBlock::Text(format!(
            "{transcript}\n\n{TITLE_PROMPT}"
        ))],
        tool_call_id: None,
    };
    let mut stream = provider::stream_chat(
        client,
        model,
        TITLE_SYSTEM_PROMPT.to_string(),
        vec![message],
        Vec::new(),
        256,
    );
    let mut text = String::new();
    let mut usage = None;
    while let Some(delta) = stream.next().await {
        match delta {
            ProviderDelta::TextDelta { text: delta, .. } => text.push_str(&delta),
            ProviderDelta::TextEnd { text: end, .. } => {
                if text.is_empty() {
                    text = end;
                }
            }
            ProviderDelta::Usage {
                input,
                output,
                cache_read,
                cache_write,
                ..
            } => {
                usage = Some(json!({
                    "input": input,
                    "output": output,
                    "cacheRead": cache_read,
                    "cacheWrite": cache_write,
                    "total": input + output,
                }));
            }
            ProviderDelta::Error { message } => return Err(anyhow!(message)),
            ProviderDelta::Done => break,
            _ => {}
        }
    }
    if text.trim().is_empty() {
        return Err(anyhow!("The model did not return a session title"));
    }
    Ok(GeneratedTitle {
        title: parse_title(&text)?,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_clips_long_sessions_and_keeps_both_ends() {
        let mut entries = Vec::new();
        for index in 0..40 {
            entries.push(json!({
                "type": "message",
                "message": {"role": if index % 2 == 0 {"user"} else {"assistant"}, "content": format!("line {index} {}", "x".repeat(400))}
            }));
        }
        let transcript = build_transcript(&entries);
        assert!(transcript.chars().count() <= TRANSCRIPT_CHARS + 20);
        assert!(transcript.contains("line 0 "));
        assert!(transcript.contains("line 39 "));
        assert!(transcript.contains("[…]"));
    }

    #[test]
    fn parses_plain_json_fenced_and_quoted_titles() {
        assert_eq!(parse_title("Fix login bug").unwrap(), "Fix login bug");
        assert_eq!(
            parse_title("Title: \"Fix login bug.\"").unwrap(),
            "Fix login bug"
        );
        assert_eq!(
            parse_title("```json\n{\"title\":\"修复登录\"}\n```").unwrap(),
            "修复登录"
        );
        assert_eq!(parse_title("「修复登录」").unwrap(), "修复登录");
        assert!(parse_title("   ").is_err());
    }
}
