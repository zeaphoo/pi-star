//! Minimal MCP host for the Rust agent: reads the same `mcp.json` files as
//! pi-web, connects stdio/HTTP servers and registers their tools as
//! `mcp__<server>__<tool>`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::config::Config;
use crate::settings;

const PROTOCOL_VERSION: &str = "2024-11-05";
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub server: String,
    pub tool: String,
}

#[derive(Debug, Clone, Default)]
struct ServerConfig {
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    url: Option<String>,
    headers: BTreeMap<String, String>,
    enabled: bool,
    exposure: String,
}

impl ServerConfig {
    fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let env = object
            .and_then(|object| object.get("env"))
            .and_then(Value::as_object)
            .map(|env| {
                env.iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let headers = object
            .and_then(|object| object.get("headers"))
            .and_then(Value::as_object)
            .map(|headers| {
                headers
                    .iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            command: object
                .and_then(|object| object.get("command"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            args: object
                .and_then(|object| object.get("args"))
                .and_then(Value::as_array)
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            env,
            url: object
                .and_then(|object| object.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            headers,
            enabled: object
                .and_then(|object| object.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            exposure: object
                .and_then(|object| object.get("exposure"))
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_string(),
        }
    }
}

fn read_servers(path: &Path) -> BTreeMap<String, ServerConfig> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return BTreeMap::new();
    };
    value
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .map(|(name, value)| (name.clone(), ServerConfig::from_value(value)))
                .collect()
        })
        .unwrap_or_default()
}

fn configured_servers(config: &Config, cwd: &Path) -> BTreeMap<String, ServerConfig> {
    let mut servers = read_servers(&config.agent_dir.join("mcp.json"));
    if settings::project_trust(config, cwd).trusted {
        servers.extend(read_servers(&cwd.join(".pi").join("mcp.json")));
    }
    servers.retain(|_, server| {
        server.enabled
            && server.exposure != "codemode"
            && (server.command.is_some() || server.url.is_some())
    });
    servers
}

pub(crate) fn has_configured_servers(config: &Config, cwd: &Path) -> bool {
    !configured_servers(config, cwd).is_empty()
}

fn tool_name(server: &str, tool: &str) -> String {
    let sanitize = |value: &str| {
        value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    format!("mcp__{}__{}", sanitize(server), sanitize(tool))
}

enum Transport {
    Stdio {
        _child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
    Http {
        client: reqwest::Client,
        url: String,
        headers: BTreeMap<String, String>,
    },
}

struct Connection {
    server: String,
    transport: Transport,
    next_id: u64,
}

impl Connection {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        });
        match &mut self.transport {
            Transport::Stdio { stdin, stdout, .. } => {
                let mut line = request.to_string();
                line.push('\n');
                stdin.write_all(line.as_bytes()).await?;
                stdin.flush().await?;
                let read = tokio::time::timeout(REQUEST_TIMEOUT, async {
                    loop {
                        let mut line = String::new();
                        if stdout.read_line(&mut line).await? == 0 {
                            return Err(anyhow!("MCP server closed its stdout"));
                        }
                        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
                            continue;
                        };
                        if value.get("id") != Some(&json!(self.next_id)) {
                            continue;
                        }
                        if let Some(error) = value.get("error") {
                            return Err(anyhow!(error.to_string()));
                        }
                        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                    }
                })
                .await
                .map_err(|_| anyhow!("MCP request timed out"))??;
                Ok(read)
            }
            Transport::Http {
                client,
                url,
                headers,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .header("Accept", "application/json, text/event-stream")
                    .header("Content-Type", "application/json");
                for (key, value) in headers.iter() {
                    request = request.header(key.as_str(), value.as_str());
                }
                let response = request
                    .json(&request_json(method, self.next_id, params))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    return Err(anyhow!("MCP HTTP {}", response.status()));
                }
                let text = response.text().await?;
                let value: Value =
                    if text.trim_start().starts_with("event:") || text.contains("data:") {
                        text.lines()
                            .filter_map(|line| line.strip_prefix("data:").map(str::trim))
                            .filter(|line| !line.is_empty())
                            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                            .find(|value| value.get("id") == Some(&json!(self.next_id)))
                            .unwrap_or(Value::Null)
                    } else {
                        serde_json::from_str(&text).unwrap_or(Value::Null)
                    };
                if let Some(error) = value.get("error") {
                    return Err(anyhow!(error.to_string()));
                }
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let notification = json!({"jsonrpc": "2.0", "method": method, "params": params});
        if let Transport::Stdio { stdin, .. } = &mut self.transport {
            let mut line = notification.to_string();
            line.push('\n');
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
        }
        Ok(())
    }

    async fn initialize(&mut self) -> Result<()> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "pi-star", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;
        self.notify("notifications/initialized", json!({})).await
    }

    async fn list_tools(&mut self) -> Result<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let result = self
                .request(
                    "tools/list",
                    cursor
                        .as_ref()
                        .map_or_else(|| json!({}), |cursor| json!({"cursor": cursor})),
                )
                .await?;
            if let Some(items) = result.get("tools").and_then(Value::as_array) {
                tools.extend(items.iter().cloned());
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String> {
        let result = self
            .request("tools/call", json!({"name": name, "arguments": arguments}))
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(anyhow!(result.to_string()));
        }
        let mut text = String::new();
        if let Some(content) = result.get("content").and_then(Value::as_array) {
            for block in content {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(value) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(value);
                        }
                    }
                    _ => {}
                }
            }
        }
        if text.is_empty() {
            text = result.to_string();
        }
        Ok(text)
    }
}

fn request_json(method: &str, id: u64, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub struct McpRuntime {
    connections: Vec<Connection>,
    pub tools: Vec<McpTool>,
}

impl McpRuntime {
    pub async fn connect(config: &Config, cwd: &Path) -> Self {
        let mut connections = Vec::new();
        let mut tools = Vec::new();
        for (server_name, server) in configured_servers(config, cwd) {
            let mut connection = match Self::open(&server_name, &server, cwd).await {
                Ok(connection) => connection,
                Err(error) => {
                    tracing::warn!(server = %server_name, %error, "MCP server did not connect");
                    continue;
                }
            };
            let list = match connection.list_tools().await {
                Ok(tools) => tools,
                Err(error) => {
                    tracing::warn!(server = %server_name, %error, "MCP tools/list failed");
                    continue;
                }
            };
            for tool in list {
                let Some(tool_id) = tool.get("name").and_then(Value::as_str).map(str::to_owned)
                else {
                    continue;
                };
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let parameters = tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                tools.push(McpTool {
                    name: tool_name(&server_name, &tool_id),
                    description,
                    parameters,
                    server: server_name.clone(),
                    tool: tool_id,
                });
            }
            connections.push(connection);
        }
        Self { connections, tools }
    }

    async fn open(name: &str, server: &ServerConfig, cwd: &Path) -> Result<Connection> {
        let transport = if let Some(url) = &server.url {
            Transport::Http {
                client: reqwest::Client::builder()
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                    .unwrap_or_default(),
                url: url.clone(),
                headers: server.headers.clone(),
            }
        } else {
            let command = server
                .command
                .as_ref()
                .ok_or_else(|| anyhow!("missing command"))?;
            let mut child = Command::new(command);
            child
                .args(&server.args)
                .current_dir(cwd)
                .envs(&server.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = child.spawn()?;
            let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
            Transport::Stdio {
                _child: child,
                stdin,
                stdout: BufReader::new(stdout),
            }
        };
        let mut connection = Connection {
            server: name.to_string(),
            transport,
            next_id: 0,
        };
        connection.initialize().await?;
        Ok(connection)
    }

    pub async fn call(&mut self, name: &str, arguments: Value) -> Result<String> {
        let Some(tool) = self.tools.iter().find(|tool| tool.name == name).cloned() else {
            return Err(anyhow!("Unknown MCP tool: {name}"));
        };
        let Some(connection) = self
            .connections
            .iter_mut()
            .find(|connection| connection.server == tool.server)
        else {
            return Err(anyhow!("MCP server is not connected: {}", tool.server));
        };
        connection.call_tool(&tool.tool, arguments).await
    }

    pub fn tool_schemas(&self) -> Vec<(String, String, Value)> {
        self.tools
            .iter()
            .map(|tool| {
                (
                    tool.name.clone(),
                    tool.description.clone(),
                    tool.parameters.clone(),
                )
            })
            .collect()
    }
}

// Drop closes stdio children through `kill_on_drop`; HTTP needs nothing.
