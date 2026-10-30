use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, anyhow};
use globset::Glob;
use regex::Regex;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::file_access::AllowedRoots;
use crate::files;

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub details: Value,
}

pub fn schemas() -> Vec<ToolSchema> {
    vec![
        schema(
            "read",
            "Read a text file from the project.",
            json!({
                "type":"object","properties":{
                    "path":{"type":"string","description":"Absolute or project-relative path"},
                    "offset":{"type":"number","description":"1-based line offset"},
                    "limit":{"type":"number","description":"Maximum lines"}
                },"required":["path"]
            }),
        ),
        schema(
            "write",
            "Write a complete file, creating parent directories.",
            json!({
                "type":"object","properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"}
                },"required":["path","content"]
            }),
        ),
        schema(
            "edit",
            "Replace exact text in a file.",
            json!({
                "type":"object","properties":{
                    "path":{"type":"string"},
                    "oldText":{"type":"string"},
                    "newText":{"type":"string"},
                    "edits":{"type":"array","items":{"type":"object","properties":{"oldText":{"type":"string"},"newText":{"type":"string"}}}}
                },"required":["path"]
            }),
        ),
        schema(
            "bash",
            "Run a shell command in the project directory.",
            json!({
                "type":"object","properties":{
                    "command":{"type":"string"},
                    "timeout":{"type":"number","description":"Timeout in milliseconds"}
                },"required":["command"]
            }),
        ),
        schema(
            "find",
            "Find files matching a glob pattern.",
            json!({
                "type":"object","properties":{
                    "pattern":{"type":"string"},
                    "path":{"type":"string","description":"Directory to search, defaults to the project"}
                },"required":["pattern"]
            }),
        ),
        schema(
            "grep",
            "Search file contents with a regular expression.",
            json!({
                "type":"object","properties":{
                    "pattern":{"type":"string"},
                    "path":{"type":"string"},
                    "glob":{"type":"string"}
                },"required":["pattern"]
            }),
        ),
        schema(
            "ls",
            "List a directory.",
            json!({
                "type":"object","properties":{"path":{"type":"string"}},"required":["path"]
            }),
        ),
        schema(
            "subagent",
            "Run a child coding agent and return its final answer.",
            json!({
                "type":"object","properties":{
                    "description":{"type":"string","description":"Short label shown in the session tree"},
                    "prompt":{"type":"string","description":"Complete instruction for the child agent"}
                },"required":["description","prompt"]
            }),
        ),
    ]
}

fn schema(name: &str, description: &str, parameters: Value) -> ToolSchema {
    ToolSchema {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    }
}

pub fn names() -> Vec<String> {
    schemas().into_iter().map(|schema| schema.name).collect()
}

pub async fn execute(
    roots: &AllowedRoots,
    cwd: &Path,
    name: &str,
    args: &Value,
    on_update: impl Fn(&str),
) -> ToolOutput {
    let result = match name {
        "read" => read(roots, cwd, args),
        "write" => write(roots, cwd, args),
        "edit" => edit(roots, cwd, args),
        "ls" => ls(roots, cwd, args),
        "find" | "glob" => find(roots, cwd, args),
        "grep" => grep(roots, cwd, args),
        "bash" => bash(roots, cwd, args, on_update).await,
        other => Err(anyhow!("unknown tool: {other}")),
    };
    match result {
        Ok(output) => output,
        Err(error) => ToolOutput {
            content: error.to_string(),
            is_error: true,
            details: json!({}),
        },
    }
}

fn resolve(roots: &AllowedRoots, cwd: &Path, args: &Value, key: &str) -> Result<PathBuf> {
    let raw = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{key} is required"))?;
    let path = Path::new(raw);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let path = crate::file_access::normalize_lexical(&path);
    if !roots.is_allowed(&path) {
        return Err(anyhow!("Access denied: {raw}"));
    }
    Ok(path)
}

fn read(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let path = resolve(roots, cwd, args, "path")?;
    if !roots.is_allowed_existing(&path) {
        return Err(anyhow!("Access denied"));
    }
    let metadata = std::fs::metadata(&path)?;
    if !metadata.is_file() {
        return Err(anyhow!("Not a file"));
    }
    if metadata.len() > files::TEXT_PREVIEW_MAX_BYTES * 4 {
        return Err(anyhow!("File too large to read (>1MB)"));
    }
    let text = std::fs::read_to_string(&path)?;
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(2000) as usize;
    let lines: Vec<&str> = text.lines().collect();
    let selected = lines
        .iter()
        .skip(offset - 1)
        .take(limit)
        .enumerate()
        .map(|(index, line)| format!("{:>6}\t{line}", offset + index))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutput {
        content: if selected.is_empty() {
            format!("(empty file: {})", path.display())
        } else {
            selected
        },
        is_error: false,
        details: json!({ "path": path, "size": metadata.len() }),
    })
}

fn write(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let path = resolve(roots, cwd, args, "path")?;
    if let Some(parent) = path.parent() {
        let parent = crate::file_access::resolve_existing(parent);
        if !roots.is_allowed(&parent) {
            return Err(anyhow!("Access denied"));
        }
        std::fs::create_dir_all(parent)?;
    }
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("content is required"))?;
    std::fs::write(&path, content)?;
    Ok(ToolOutput {
        content: format!("Wrote {} bytes to {}", content.len(), path.display()),
        is_error: false,
        details: json!({ "path": path, "bytes": content.len() }),
    })
}

fn edit(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let path = resolve(roots, cwd, args, "path")?;
    if !roots.is_allowed_existing(&path) {
        return Err(anyhow!("Access denied"));
    }
    let mut text = std::fs::read_to_string(&path)?;
    let edits: Vec<(String, String)> =
        if let Some(items) = args.get("edits").and_then(Value::as_array) {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("oldText")?.as_str()?.to_string(),
                        item.get("newText")?.as_str().unwrap_or("").to_string(),
                    ))
                })
                .collect()
        } else {
            vec![(
                args.get("oldText")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                args.get("newText")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )]
        };
    for (old, new) in &edits {
        if old.is_empty() {
            return Err(anyhow!("oldText is required"));
        }
        let Some(index) = text.find(old) else {
            return Err(anyhow!("oldText not found in {}", path.display()));
        };
        text.replace_range(index..index + old.len(), new);
    }
    std::fs::write(&path, &text)?;
    Ok(ToolOutput {
        content: format!("Applied {} edit(s) to {}", edits.len(), path.display()),
        is_error: false,
        details: json!({ "path": path, "edits": edits.len() }),
    })
}

fn ls(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let path = if args.get("path").and_then(Value::as_str).is_some() {
        resolve(roots, cwd, args, "path")?
    } else {
        cwd.to_path_buf()
    };
    let list = files::list_directory(&path, true)?;
    let lines = list
        .entries
        .iter()
        .map(|entry| format!("{}{}", entry.name, if entry.is_dir { "/" } else { "" }))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutput {
        content: lines,
        is_error: false,
        details: json!({ "path": path }),
    })
}

fn find(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("pattern is required"))?;
    let glob = Glob::new(pattern)?.compile_matcher();
    let base = if args.get("path").and_then(Value::as_str).is_some() {
        resolve(roots, cwd, args, "path")?
    } else {
        cwd.to_path_buf()
    };
    let mut matches = Vec::new();
    for entry in ignore::WalkBuilder::new(&base)
        .hidden(false)
        .git_ignore(true)
        .max_depth(Some(12))
        .build()
        .flatten()
    {
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            let path = entry.path();
            let relative = path.strip_prefix(&base).unwrap_or(path);
            if glob.is_match(relative) {
                matches.push(relative.to_string_lossy().into_owned());
                if matches.len() >= 500 {
                    break;
                }
            }
        }
    }
    Ok(ToolOutput {
        content: if matches.is_empty() {
            format!("No files match {pattern}")
        } else {
            matches.join("\n")
        },
        is_error: false,
        details: json!({ "pattern": pattern, "count": matches.len() }),
    })
}

fn grep(roots: &AllowedRoots, cwd: &Path, args: &Value) -> Result<ToolOutput> {
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("pattern is required"))?;
    let regex = Regex::new(pattern)?;
    let glob = args
        .get("glob")
        .and_then(Value::as_str)
        .map(Glob::new)
        .transpose()?
        .map(|glob| glob.compile_matcher());
    let base = if args.get("path").and_then(Value::as_str).is_some() {
        resolve(roots, cwd, args, "path")?
    } else {
        cwd.to_path_buf()
    };
    let mut output = Vec::new();
    for entry in ignore::WalkBuilder::new(&base)
        .hidden(false)
        .git_ignore(true)
        .max_depth(Some(12))
        .build()
        .flatten()
    {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(&base).unwrap_or(path);
        if glob.as_ref().is_some_and(|glob| !glob.is_match(relative)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                output.push(format!(
                    "{}:{}: {}",
                    relative.display(),
                    index + 1,
                    line.trim_end()
                ));
                if output.len() >= 500 {
                    break;
                }
            }
        }
        if output.len() >= 500 {
            break;
        }
    }
    Ok(ToolOutput {
        content: if output.is_empty() {
            format!("No matches for {pattern}")
        } else {
            output.join("\n")
        },
        is_error: false,
        details: json!({ "pattern": pattern, "count": output.len() }),
    })
}

async fn bash(
    roots: &AllowedRoots,
    cwd: &Path,
    args: &Value,
    on_update: impl Fn(&str),
) -> Result<ToolOutput> {
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("command is required"))?;
    if !roots.is_allowed(cwd) {
        return Err(anyhow!("Access denied"));
    }
    let timeout = args
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(120_000)
        .clamp(1_000, 600_000);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut child = Command::new(shell)
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut collected = String::new();
        if let Some(mut stdout) = stdout {
            let mut buffer = [0u8; 8192];
            loop {
                match stdout.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        collected.push_str(&String::from_utf8_lossy(&buffer[..n]));
                        if collected.len() > 1024 * 1024 {
                            collected.truncate(1024 * 1024);
                            collected.push_str("\n… output truncated");
                            break;
                        }
                    }
                }
            }
        }
        if let Some(mut stderr) = stderr {
            let mut buffer = [0u8; 8192];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        collected.push_str(&String::from_utf8_lossy(&buffer[..n]));
                        if collected.len() > 1024 * 1024 {
                            collected.truncate(1024 * 1024);
                            collected.push_str("\n… output truncated");
                            break;
                        }
                    }
                }
            }
        }
        collected
    });
    match tokio::time::timeout(Duration::from_millis(timeout), child.wait()).await {
        Ok(Ok(status)) => {
            let output = read_task.await.unwrap_or_default();
            on_update(&output.chars().take(4000).collect::<String>());
            Ok(ToolOutput {
                content: if output.is_empty() {
                    format!("(no output, exit {})", status.code().unwrap_or(-1))
                } else {
                    output
                },
                is_error: !status.success(),
                details: json!({ "exitCode": status.code().unwrap_or(-1), "command": command }),
            })
        }
        Ok(Err(error)) => Err(error.into()),
        Err(_) => {
            let _ = child.kill().await;
            Err(anyhow!("command timed out after {timeout} ms"))
        }
    }
}
