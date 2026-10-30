use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use async_stream::stream;
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};

use crate::models::ModelDef;

#[derive(Debug, Clone)]
pub enum ProviderDelta {
    TextStart {
        index: usize,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    TextEnd {
        index: usize,
        text: String,
    },
    ThinkingStart {
        index: usize,
    },
    ThinkingDelta {
        index: usize,
        text: String,
    },
    ThinkingEnd {
        index: usize,
        text: String,
    },
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        delta: String,
    },
    ToolCallEnd {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
    Usage {
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        cost: f64,
    },
    Error {
        message: String,
    },
    Done,
}

pub type DeltaStream = std::pin::Pin<Box<dyn Stream<Item = ProviderDelta> + Send>>;

#[derive(Debug, Clone)]
pub struct ProviderMessage {
    pub role: String,
    pub content: Vec<ProviderBlock>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ProviderBlock {
    Text(String),
    Image {
        media_type: String,
        data: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
    Thinking(String),
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub fn is_anthropic(model: &ModelDef) -> bool {
    model.api.contains("anthropic") || model.base_url.contains("anthropic")
}

pub fn stream_chat(
    client: reqwest::Client,
    model: ModelDef,
    system: String,
    messages: Vec<ProviderMessage>,
    tools: Vec<ToolSchema>,
    max_tokens: u64,
) -> DeltaStream {
    if is_anthropic(&model) {
        anthropic_stream(client, model, system, messages, tools, max_tokens)
    } else {
        openai_stream(client, model, system, messages, tools, max_tokens)
    }
}

fn openai_messages(system: &str, messages: &[ProviderMessage]) -> Vec<Value> {
    let mut result = Vec::new();
    if !system.is_empty() {
        result.push(json!({"role":"system","content":system}));
    }
    for message in messages {
        match message.role.as_str() {
            "toolResult" => {
                let content = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ProviderBlock::Text(text) => Some(text.clone()),
                        ProviderBlock::ToolResult { content, .. } => Some(content.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                result.push(json!({
                    "role": "tool",
                    "tool_call_id": message.tool_call_id.clone().unwrap_or_default(),
                    "content": content,
                }));
            }
            "assistant" => {
                let mut text = String::new();
                let mut tool_calls = Vec::new();
                let mut thinking = String::new();
                for block in &message.content {
                    match block {
                        ProviderBlock::Text(value) => text.push_str(value),
                        ProviderBlock::Thinking(value) => thinking.push_str(value),
                        ProviderBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": arguments.to_string() }
                        })),
                        _ => {}
                    }
                }
                let mut value = json!({"role":"assistant"});
                if !text.is_empty() || tool_calls.is_empty() {
                    value["content"] = Value::String(text);
                } else {
                    value["content"] = Value::Null;
                }
                if !thinking.is_empty() {
                    value["reasoning_content"] = Value::String(thinking);
                }
                if !tool_calls.is_empty() {
                    value["tool_calls"] = Value::Array(tool_calls);
                }
                result.push(value);
            }
            _ => {
                let content: Vec<Value> = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ProviderBlock::Text(text) => Some(json!({"type":"text","text":text})),
                        ProviderBlock::Image { media_type, data } => Some(json!({
                            "type":"image_url",
                            "image_url": {"url": format!("data:{media_type};base64,{data}")}
                        })),
                        _ => None,
                    })
                    .collect();
                if content.len() == 1
                    && content[0].get("type").and_then(Value::as_str) == Some("text")
                {
                    result.push(json!({"role":"user","content": content[0]["text"]}));
                } else {
                    result.push(json!({"role":"user","content": content}));
                }
            }
        }
    }
    result
}

fn openai_stream(
    client: reqwest::Client,
    model: ModelDef,
    system: String,
    messages: Vec<ProviderMessage>,
    tools: Vec<ToolSchema>,
    max_tokens: u64,
) -> DeltaStream {
    Box::pin(stream! {
        let base = model.base_url.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        };
        let mut body = json!({
            "model": model.id,
            "messages": openai_messages(&system, &messages),
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if max_tokens > 0 {
            body["max_tokens"] = Value::Number(max_tokens.into());
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(|tool| json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            })).collect());
            body["tool_choice"] = Value::String("auto".into());
        }
        let mut request = client.post(&url).json(&body);
        if let Some(key) = &model.api_key {
            request = request.bearer_auth(key);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                yield ProviderDelta::Error { message: format!("request failed: {error}") };
                yield ProviderDelta::Done;
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let text = text.chars().take(2000).collect::<String>();
            yield ProviderDelta::Error { message: format!("provider HTTP {status}: {text}") };
            yield ProviderDelta::Done;
            return;
        }
        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut text_index: Option<usize> = None;
        let mut text = String::new();
        let mut thinking_index: Option<usize> = None;
        let mut thinking = String::new();
        let mut tools: BTreeMap<usize, (String, String, String, usize)> = BTreeMap::new();
        let mut next_index = 0usize;
        while let Some(chunk) = byte_stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield ProviderDelta::Error { message: format!("stream failed: {error}") };
                    break;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            loop {
                let Some(newline) = buffer.find('\n') else { break };
                let line = buffer[..newline].trim_end_matches('\r').to_string();
                buffer.drain(..=newline);
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(data) else { continue };
                if let Some(usage) = value.get("usage") {
                    yield ProviderDelta::Usage {
                        input: usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                        output: usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
                        cache_read: usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or(0),
                        cache_write: 0,
                        cost: 0.0,
                    };
                }
                let Some(delta) = value.pointer("/choices/0/delta") else { continue };
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                    if !reasoning.is_empty() {
                        let index = match thinking_index {
                            Some(index) => index,
                            None => {
                                let index = next_index;
                                next_index += 1;
                                thinking_index = Some(index);
                                yield ProviderDelta::ThinkingStart { index };
                                index
                            }
                        };
                        thinking.push_str(reasoning);
                        yield ProviderDelta::ThinkingDelta { index, text: reasoning.to_string() };
                    }
                }
                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        let index = match text_index {
                            Some(index) => index,
                            None => {
                                let index = next_index;
                                next_index += 1;
                                text_index = Some(index);
                                yield ProviderDelta::TextStart { index };
                                index
                            }
                        };
                        text.push_str(content);
                        yield ProviderDelta::TextDelta { index, text: content.to_string() };
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tools.entry(index).or_insert_with(|| (String::new(), String::new(), String::new(), usize::MAX));
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            entry.0 = id.to_string();
                        }
                        if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                            entry.1 = name.to_string();
                        }
                        if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
                            if entry.0.is_empty() {
                                entry.0 = format!("call_{index}");
                            }
                            if entry.3 == usize::MAX {
                                entry.3 = next_index;
                                next_index += 1;
                                yield ProviderDelta::ToolCallStart {
                                    index: entry.3,
                                    id: entry.0.clone(),
                                    name: entry.1.clone(),
                                };
                            }
                            entry.2.push_str(arguments);
                            yield ProviderDelta::ToolCallDelta {
                                index: entry.3,
                                delta: arguments.to_string(),
                            };
                        }
                    }
                }
            }
        }
        if let Some(index) = thinking_index {
            yield ProviderDelta::ThinkingEnd { index, text: thinking };
        }
        if let Some(index) = text_index {
            yield ProviderDelta::TextEnd { index, text };
        }
        for (_, (id, name, arguments, index)) in tools {
            if index != usize::MAX {
                yield ProviderDelta::ToolCallEnd { index, id, name, arguments };
            }
        }
        yield ProviderDelta::Done;
    })
}

fn anthropic_messages(system: &str, messages: &[ProviderMessage]) -> (String, Vec<Value>) {
    let mut result = Vec::new();
    for message in messages {
        match message.role.as_str() {
            "toolResult" => {
                let content = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ProviderBlock::ToolResult { content, .. } => Some(content.clone()),
                        ProviderBlock::Text(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let id = message.tool_call_id.clone().unwrap_or_default();
                result.push(json!({
                    "role": "user",
                    "content": [{"type":"tool_result","tool_use_id":id,"content":content}]
                }));
            }
            "assistant" => {
                let content: Vec<Value> = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ProviderBlock::Text(text) => Some(json!({"type":"text","text":text})),
                        ProviderBlock::Thinking(text) => {
                            Some(json!({"type":"thinking","thinking":text}))
                        }
                        ProviderBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(json!({
                            "type":"tool_use","id":id,"name":name,"input":arguments
                        })),
                        _ => None,
                    })
                    .collect();
                result.push(json!({"role":"assistant","content":content}));
            }
            _ => {
                let content: Vec<Value> = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ProviderBlock::Text(text) => Some(json!({"type":"text","text":text})),
                        ProviderBlock::Image { media_type, data } => Some(json!({
                            "type":"image",
                            "source":{"type":"base64","media_type":media_type,"data":data}
                        })),
                        _ => None,
                    })
                    .collect();
                result.push(json!({"role":"user","content":content}));
            }
        }
    }
    (system.to_string(), result)
}

fn anthropic_stream(
    client: reqwest::Client,
    model: ModelDef,
    system: String,
    messages: Vec<ProviderMessage>,
    tools: Vec<ToolSchema>,
    max_tokens: u64,
) -> DeltaStream {
    Box::pin(stream! {
        let base = model.base_url.trim_end_matches('/');
        let url = if base.ends_with("/v1/messages") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        };
        let (system, messages) = anthropic_messages(&system, &messages);
        let mut body = json!({
            "model": model.id,
            "max_tokens": if max_tokens > 0 { max_tokens } else { 8192 },
            "messages": messages,
            "stream": true,
            "thinking": {"type": "enabled", "budget_tokens": 4096},
        });
        if !system.is_empty() {
            body["system"] = Value::String(system);
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters,
            })).collect());
        }
        let mut request = client
            .post(&url)
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        if let Some(key) = &model.api_key {
            request = request.header("x-api-key", key);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                yield ProviderDelta::Error { message: format!("request failed: {error}") };
                yield ProviderDelta::Done;
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let text = text.chars().take(2000).collect::<String>();
            yield ProviderDelta::Error { message: format!("provider HTTP {status}: {text}") };
            yield ProviderDelta::Done;
            return;
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut blocks: BTreeMap<usize, (String, String, String, String)> = BTreeMap::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield ProviderDelta::Error { message: format!("stream failed: {error}") };
                    break;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            loop {
                let Some(newline) = buffer.find('\n') else { break };
                let line = buffer[..newline].trim_end_matches('\r').to_string();
                buffer.drain(..=newline);
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                let Ok(value) = serde_json::from_str::<Value>(data) else { continue };
                let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
                match event_type {
                    "content_block_start" => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let block = value.get("content_block").cloned().unwrap_or(Value::Null);
                        match block.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                blocks.insert(index, ("text".into(), String::new(), String::new(), String::new()));
                                yield ProviderDelta::TextStart { index };
                            }
                            Some("thinking") => {
                                blocks.insert(index, ("thinking".into(), String::new(), String::new(), String::new()));
                                yield ProviderDelta::ThinkingStart { index };
                            }
                            Some("tool_use") => {
                                let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                                let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                                blocks.insert(index, ("tool".into(), id.clone(), name.clone(), String::new()));
                                yield ProviderDelta::ToolCallStart { index, id, name };
                            }
                            _ => {}
                        }
                    }
                    "content_block_delta" => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let delta = value.get("delta").cloned().unwrap_or(Value::Null);
                        match delta.get("type").and_then(Value::as_str) {
                            Some("text_delta") => {
                                let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                                if let Some(block) = blocks.get_mut(&index) {
                                    block.1.push_str(text);
                                }
                                yield ProviderDelta::TextDelta { index, text: text.to_string() };
                            }
                            Some("thinking_delta") => {
                                let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                                if let Some(block) = blocks.get_mut(&index) {
                                    block.1.push_str(text);
                                }
                                yield ProviderDelta::ThinkingDelta { index, text: text.to_string() };
                            }
                            Some("input_json_delta") => {
                                let text = delta.get("partial_json").and_then(Value::as_str).unwrap_or("");
                                if let Some(block) = blocks.get_mut(&index) {
                                    block.1.push_str(text);
                                }
                                yield ProviderDelta::ToolCallDelta { index, delta: text.to_string() };
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some((kind, text, id, name)) = blocks.remove(&index) {
                            match kind.as_str() {
                                "text" => yield ProviderDelta::TextEnd { index, text },
                                "thinking" => yield ProviderDelta::ThinkingEnd { index, text },
                                "tool" => {
                                    yield ProviderDelta::ToolCallEnd {
                                        index,
                                        id,
                                        name,
                                        arguments: text,
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(usage) = value.get("usage") {
                            yield ProviderDelta::Usage {
                                input: usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
                                output: usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
                                cache_read: usage.get("cache_read_input_tokens").and_then(Value::as_u64).unwrap_or(0),
                                cache_write: usage.get("cache_creation_input_tokens").and_then(Value::as_u64).unwrap_or(0),
                                cost: 0.0,
                            };
                        }
                    }
                    "error" => {
                        let message = value.pointer("/error/message").and_then(Value::as_str).unwrap_or("provider error");
                        yield ProviderDelta::Error { message: message.to_string() };
                    }
                    _ => {}
                }
            }
        }
        yield ProviderDelta::Done;
    })
}

pub fn estimate_cost(
    model: &ModelDef,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
) -> f64 {
    let cost = &model.cost;
    let number = |key: &str| cost.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    (input as f64 * number("input")
        + output as f64 * number("output")
        + cache_read as f64 * number("cacheRead")
        + cache_write as f64 * number("cacheWrite"))
        / 1_000_000.0
}

pub fn parse_arguments(text: &str) -> Result<Value> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text).map_err(|error| anyhow!("invalid tool arguments: {error}"))
}
