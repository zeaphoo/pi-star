use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TEXT_PREVIEW_MAX_BYTES: u64 = 256 * 1024;
pub const IMAGE_PREVIEW_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_UPLOAD_FILE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_UPLOAD_TOTAL_BYTES: usize = 100 * 1024 * 1024;
const MAX_INDEX_FILES: usize = 5000;
const MAX_INDEX_WALK: usize = 50_000;
const MAX_WALK_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside_link_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside_link_encloses: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileList {
    pub entries: Vec<FileEntry>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextChunk {
    pub content: String,
    pub next_offset: u64,
    pub truncated: bool,
    pub binary: bool,
    pub language: String,
    pub size: u64,
    pub mtime: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub size: u64,
    pub mtime: String,
    pub language: String,
    pub mime: String,
    pub preview_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndexEntry {
    pub path: String,
    pub is_dir: bool,
}

pub fn list_directory(dir: &Path, show_hidden: bool) -> Result<FileList> {
    let metadata = fs::metadata(dir).with_context(|| format!("stat {}", dir.display()))?;
    if !metadata.is_dir() {
        return Err(anyhow!("Not a directory"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let is_symlink = metadata.file_type().is_symlink();
        let target_metadata = if is_symlink {
            fs::metadata(&path).ok()
        } else {
            Some(metadata.clone())
        };
        let Some(target_metadata) = target_metadata else {
            continue;
        };
        let is_dir = target_metadata.is_dir();
        let hidden = if !show_hidden && is_hidden_name(&name) {
            continue;
        } else {
            None
        };
        entries.push(FileEntry {
            name,
            is_dir,
            size: if is_dir { 0 } else { target_metadata.len() },
            modified: target_metadata
                .modified()
                .ok()
                .map(crate::session::format_time)
                .unwrap_or_default(),
            hidden,
            outside_link_target: None,
            outside_link_encloses: None,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(FileList {
        entries,
        path: dir.to_string_lossy().into_owned(),
    })
}

fn is_hidden_name(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | ".git"
            | ".next"
            | "dist"
            | "build"
            | "__pycache__"
            | ".turbo"
            | ".cache"
            | "coverage"
            | ".pytest_cache"
            | ".mypy_cache"
            | "target"
            | "vendor"
            | ".DS_Store"
    )
}

pub fn file_meta(path: &Path) -> Result<FileMeta> {
    let metadata = fs::metadata(path)?;
    let language = language_for(path).to_string();
    // Every text file reports text/plain, as pi-web does: a guessed type such
    // as `video/vnd.dlna.mpeg-tts` for `.ts` says nothing useful about source.
    let mime = media_mime_for(path).unwrap_or("text/plain").to_string();
    let preview_kind = match extension(path).as_deref() {
        Some("pdf") => Some("pdf".to_string()),
        Some("docx") => Some("docx".to_string()),
        _ => None,
    };
    Ok(FileMeta {
        size: metadata.len(),
        mtime: metadata
            .modified()
            .ok()
            .map(crate::session::format_time)
            .unwrap_or_default(),
        language,
        mime,
        preview_kind,
    })
}

pub fn read_text_chunk(path: &Path, offset: u64) -> Result<TextChunk> {
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if offset > metadata.len() {
        return Err(anyhow!("Invalid text preview offset"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer =
        vec![0u8; TEXT_PREVIEW_MAX_BYTES.min(metadata.len().saturating_sub(offset)) as usize];
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    let binary = offset == 0 && looks_binary(&buffer);
    if binary {
        return Ok(TextChunk {
            content: String::new(),
            next_offset: 0,
            truncated: false,
            binary: true,
            language: language_for(path).to_string(),
            size: metadata.len(),
            mtime: metadata
                .modified()
                .ok()
                .map(crate::session::format_time)
                .unwrap_or_default(),
        });
    }
    let mut end = buffer.len();
    if offset + (end as u64) < metadata.len() {
        while end > 0 && (buffer[end - 1] & 0xc0) == 0x80 {
            end -= 1;
        }
        if end > 0 {
            let lead = buffer[end - 1];
            let needed = if (lead & 0xf8) == 0xf0 {
                4
            } else if (lead & 0xf0) == 0xe0 {
                3
            } else if (lead & 0xe0) == 0xc0 {
                2
            } else {
                1
            };
            if needed > buffer.len() - (end - 1) {
                end -= 1;
            }
        }
    }
    let next_offset = offset + end as u64;
    let content = String::from_utf8_lossy(&buffer[..end]).into_owned();
    Ok(TextChunk {
        language: language_for_content(path, &content).to_string(),
        content,
        next_offset,
        truncated: next_offset < metadata.len(),
        binary: false,
        size: metadata.len(),
        mtime: metadata
            .modified()
            .ok()
            .map(crate::session::format_time)
            .unwrap_or_default(),
    })
}

fn looks_binary(buffer: &[u8]) -> bool {
    if buffer.starts_with(&[0xff, 0xfe]) || buffer.starts_with(&[0xfe, 0xff]) {
        return false;
    }
    buffer.iter().take(8000).any(|byte| *byte == 0)
}

pub fn language_for(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        return "dockerfile";
    }
    if name == "containerfile" || name.starts_with("containerfile.") {
        return "dockerfile";
    }
    if name == ".env" || name.starts_with(".env.") {
        return "bash";
    }
    if name == "makefile" || name == "gnumakefile" {
        return "makefile";
    }
    // Files whose whole name matters. A dotfile has no extension, so the
    // extension match below can never see .gitignore, .editorconfig or a lock
    // file such as Cargo.lock.
    match name.as_str() {
        ".gitignore" | ".dockerignore" | ".npmignore" | ".eslintignore" | ".prettierignore"
        | ".bashrc" | ".bash_profile" | ".zshrc" | ".profile" => return "bash",
        ".vimrc" | ".gvimrc" => return "vim",
        ".editorconfig" | ".npmrc" | ".yarnrc" | ".gitattributes" | ".gitmodules" => {
            return "ini";
        }
        ".babelrc" | ".eslintrc" | ".prettierrc" | ".stylelintrc" => return "json",
        "cargo.lock" => return "toml",
        "cmakelists.txt" => return "cmake",
        "gemfile" | "rakefile" | "vagrantfile" | "podfile" | "brewfile" | "fastfile"
        | "guardfile" => return "ruby",
        "justfile" | "taskfile" => return "makefile",
        _ => {}
    }
    match extension(path).as_deref() {
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx") => "tsx",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "jsx",
        Some("py") => "python",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("lua") => "lua",
        Some("pl") => "perl",
        Some("r") => "r",
        Some("go") => "go",
        Some("rs") => "rust",
        Some("java") => "java",
        Some("kt" | "kts") => "kotlin",
        Some("swift") => "swift",
        Some("scala") => "scala",
        Some("groovy" | "gradle") => "groovy",
        Some("dart") => "dart",
        Some("zig") => "zig",
        Some("c" | "h") => "c",
        Some("cpp" | "cxx" | "cc" | "hpp" | "hxx") => "cpp",
        Some("cs") => "csharp",
        Some("mm") => "objectivec",
        Some("ex" | "exs") => "elixir",
        Some("hs") => "haskell",
        Some("ml") => "ocaml",
        Some("clj") => "clojure",
        Some("nix") => "nix",
        Some("html" | "htm") => "html",
        Some("vue" | "svelte" | "astro") => "markup",
        Some("hbs" | "handlebars" | "mustache") => "handlebars",
        Some("css") => "css",
        Some("scss") => "scss",
        Some("less") => "less",
        Some("json" | "jsonl" | "jsonc" | "json5") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("ini" | "cfg" | "properties") => "ini",
        Some("xml" | "plist") => "xml",
        Some("md" | "mdx") => "markdown",
        Some("tex") => "latex",
        Some("sh" | "bash" | "zsh" | "fish" | "env" | "gitignore") => "bash",
        Some("ps1" | "psm1") => "powershell",
        Some("bat" | "cmd") => "batch",
        Some("sql") => "sql",
        Some("graphql" | "gql") => "graphql",
        Some("proto") => "protobuf",
        Some("tf" | "tfvars" | "hcl") => "hcl",
        Some("diff" | "patch") => "diff",
        Some("http") => "http",
        Some("pdf") => "pdf",
        Some("docx") => "word",
        _ => "text",
    }
}

/// Language for a text chunk. The path decides most files; for an extensionless
/// script or a config file with no useful extension, the first line/content is
/// the only clue (a shebang, an XML/HTML prolog, or a JSON document).
pub fn language_for_content(path: &Path, content: &str) -> &'static str {
    let by_path = language_for(path);
    if by_path != "text" {
        return by_path;
    }

    let first_line = content.lines().next().unwrap_or("").trim_start();
    if let Some(shebang) = first_line.strip_prefix("#!") {
        let shebang = shebang.to_ascii_lowercase();
        if shebang.contains("python") {
            return "python";
        }
        if shebang.contains("node") || shebang.contains("deno") || shebang.contains("bun") {
            return "javascript";
        }
        if shebang.contains("ruby") {
            return "ruby";
        }
        if shebang.contains("perl") {
            return "perl";
        }
        if shebang.contains("php") {
            return "php";
        }
        if shebang.contains("pwsh") || shebang.contains("powershell") {
            return "powershell";
        }
        if shebang.contains("lua") {
            return "lua";
        }
        if shebang.contains("rscript") {
            return "r";
        }
        if shebang.contains("bash")
            || shebang.contains("zsh")
            || shebang.contains("fish")
            || shebang.contains("/sh")
            || shebang.contains(" sh")
        {
            return "bash";
        }
    }

    let trimmed = content.trim_start();
    if trimmed.starts_with("<?xml") {
        return "xml";
    }
    let lower = trimmed.get(..14).unwrap_or(trimmed).to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return "html";
    }
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<Value>(trimmed).is_ok()
    {
        return "json";
    }
    "text"
}

/// The MIME type of a file the browser renders from raw bytes, or None for text.
/// The table matches the viewer's own `lib/file-types.ts`, so the API and the UI
/// agree on what is an image, a soundtrack or a document.
///
/// `mime_guess` cannot make this call: it maps `.ts` to `video/vnd.dlna.mpeg-tts`
/// (MPEG transport streams), so a TypeScript file would be served raw and the
/// viewer would try to parse its source as JSON.
pub fn media_mime_for(path: &Path) -> Option<&'static str> {
    match extension(path)?.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" | "oga" | "opus" => Some("audio/ogg"),
        "m4a" => Some("audio/mp4"),
        "aac" => Some("audio/aac"),
        "flac" => Some("audio/flac"),
        "weba" => Some("audio/webm"),
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "ogv" => Some("video/ogg"),
        "pdf" => Some("application/pdf"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        _ => None,
    }
}

/// `Content-Type` for a download: the media type when the browser can render
/// it, `application/octet-stream` otherwise, as pi-web answers.
pub fn download_mime_for(path: &Path) -> String {
    media_mime_for(path)
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

pub fn index_files(cwd: &Path, query: &str) -> Result<(Vec<String>, bool)> {
    let listing = list_index(cwd)?;
    if query.trim().is_empty() {
        let truncated = listing.len() > MAX_INDEX_FILES;
        return Ok((
            listing.into_iter().take(MAX_INDEX_FILES).collect(),
            truncated,
        ));
    }
    let query = query.to_lowercase();
    let mut matches: Vec<(i32, String)> = Vec::new();
    for path in listing {
        if let Some(score) = fuzzy_score(&path, &query) {
            matches.push((score, path));
        }
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok((
        matches.into_iter().take(50).map(|(_, path)| path).collect(),
        false,
    ))
}

fn fuzzy_score(path: &str, query: &str) -> Option<i32> {
    let lower = path.to_lowercase();
    if let Some(index) = lower.rfind(query) {
        return Some(1000 - index as i32);
    }
    let mut positions = Vec::new();
    let mut from = 0usize;
    for ch in query.chars() {
        let index = lower[from..].find(ch)?;
        positions.push(from + index);
        from += index + ch.len_utf8();
    }
    let span = positions.last().unwrap_or(&0) - positions.first().unwrap_or(&0);
    Some(500 - span as i32)
}

fn list_index(cwd: &Path) -> Result<Vec<String>> {
    if let Some(files) = list_with_git(cwd) {
        return Ok(files);
    }
    let mut files = Vec::new();
    let mut queue = vec![(cwd.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, rel, depth)) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden_name(&name) || name.starts_with('.') {
                continue;
            }
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    queue.push((entry.path(), child_rel, depth + 1));
                }
            } else if metadata.is_file() {
                if files.len() >= MAX_INDEX_WALK {
                    return Ok(files);
                }
                files.push(child_rel);
            }
        }
    }
    Ok(files)
}

fn list_with_git(cwd: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.split('\0')
            .filter(|s| !s.is_empty())
            .take(MAX_INDEX_WALK)
            .map(str::to_owned)
            .collect(),
    )
}

pub fn validate_upload_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('\0') {
        return Err(anyhow!(
            "Invalid file name: {}",
            if name.is_empty() { "(empty)" } else { name }
        ));
    }
    if name.contains('/')
        || name.contains('\\')
        || Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name)
    {
        return Err(anyhow!("File names must not contain a path: {name}"));
    }
    Ok(())
}

pub fn inspect_upload_targets(dir: &Path, names: &[String]) -> (Vec<String>, Vec<String>) {
    let mut conflicts = Vec::new();
    let mut non_replaceable = Vec::new();
    for name in names {
        let destination = dir.join(name);
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                conflicts.push(name.clone());
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    non_replaceable.push(name.clone());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
    (conflicts, non_replaceable)
}

pub fn write_upload(dir: &Path, name: &str, bytes: &[u8], overwrite: bool) -> Result<PathBuf> {
    validate_upload_name(name)?;
    let destination = dir.join(name);
    if !overwrite {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        return Ok(destination);
    }
    let metadata = fs::symlink_metadata(&destination)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(anyhow!("Cannot replace a directory or symbolic link"));
    }
    let staging = dir.join(format!(".pi-upload-{}", uuid::Uuid::new_v4()));
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&staging, &destination)?;
    Ok(destination)
}

pub fn modified_version(path: &Path) -> Option<(u64, String)> {
    let metadata = fs::metadata(path).ok()?;
    let mtime: SystemTime = metadata.modified().ok()?;
    Some((metadata.len(), crate::session::format_time(mtime)))
}

pub fn list_response_json(list: &FileList) -> Value {
    json!({ "entries": list.entries, "path": list.path })
}

/// Extract the text of a DOCX into a small, self-contained HTML document.
/// Images and Word-specific layout are intentionally skipped: the preview is a
/// safe reader, not an editor.
pub fn docx_preview(path: &Path) -> Result<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    const MAX_DOCX_BYTES: u64 = 10 * 1024 * 1024;
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_DOCX_BYTES {
        return Err(anyhow!("DOCX too large for preview (>10MB)"));
    }
    let file = fs::File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| anyhow!("invalid DOCX: {error}"))?;
    let mut document = archive
        .by_name("word/document.xml")
        .map_err(|_| anyhow!("DOCX has no word/document.xml"))?;
    let mut xml = String::new();
    document.read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut body = String::new();
    let mut in_text = false;
    let mut paragraph_open = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"p" => {
                    if paragraph_open {
                        body.push_str("</p>");
                    }
                    body.push_str("<p>");
                    paragraph_open = true;
                }
                b"t" => in_text = true,
                b"br" => body.push_str("<br>"),
                b"tab" => body.push('\t'),
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"p" => {
                    body.push_str("</p>");
                    paragraph_open = false;
                }
                b"t" => in_text = false,
                _ => {}
            },
            Ok(Event::Text(text)) => {
                if in_text {
                    let value = text.decode().unwrap_or_default();
                    body.push_str(&html_escape_text(&value));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(anyhow!("invalid DOCX XML: {error}")),
            _ => {}
        }
    }
    if paragraph_open {
        body.push_str("</p>");
    }
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.docx");
    Ok(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\
         <style>body{{font-family:system-ui,sans-serif;max-width:860px;margin:2rem auto;padding:0 1.25rem;line-height:1.6;color:#1a1a1a}}\
         p{{margin:0 0 .75rem}}h1{{font-size:1.1rem;color:#555}}</style></head>\
         <body><h1>{}</h1>{}</body></html>",
        html_escape_text(title),
        html_escape_text(title),
        body
    ))
}

fn html_escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(path: &str) -> &'static str {
        language_for(Path::new(path))
    }

    #[test]
    fn maps_extensions_and_special_names() {
        assert_eq!(language("src/main.rs"), "rust");
        assert_eq!(language("src/app.tsx"), "tsx");
        assert_eq!(language("Cargo.lock"), "toml");
        assert_eq!(language(".gitignore"), "bash");
        assert_eq!(language(".editorconfig"), "ini");
        assert_eq!(language("Info.plist"), "xml");
        assert_eq!(language("CMakeLists.txt"), "cmake");
        assert_eq!(language("Gemfile"), "ruby");
        assert_eq!(language("Dockerfile.dev"), "dockerfile");
        assert_eq!(language("styles/theme.scss"), "scss");
        assert_eq!(language("proto/service.proto"), "protobuf");
        assert_eq!(language("deploy/main.tfvars"), "hcl");
    }

    #[test]
    fn only_media_paths_are_served_as_raw_bytes() {
        // `mime_guess` calls `.ts` an MPEG transport stream; the viewer needs
        // TypeScript's source as the JSON text chunk.
        assert_eq!(media_mime_for(Path::new("src/app.ts")), None);
        assert_eq!(media_mime_for(Path::new("src/app.tsx")), None);
        assert_eq!(media_mime_for(Path::new("notes.md")), None);
        assert_eq!(media_mime_for(Path::new("app.png")), Some("image/png"));
        assert_eq!(media_mime_for(Path::new("logo.svg")), Some("image/svg+xml"));
        assert_eq!(media_mime_for(Path::new("song.MP3")), Some("audio/mpeg"));
        assert_eq!(media_mime_for(Path::new("clip.mp4")), Some("video/mp4"));
        assert_eq!(
            media_mime_for(Path::new("doc.pdf")),
            Some("application/pdf")
        );
        // A download of a text file is an octet stream, never a guessed type.
        assert_eq!(
            download_mime_for(Path::new("app.ts")),
            "application/octet-stream"
        );
    }

    #[test]
    fn detects_language_from_content_when_the_path_has_none() {
        assert_eq!(
            language_for_content(Path::new("script"), "#!/usr/bin/env python3\nprint(1)"),
            "python"
        );
        assert_eq!(
            language_for_content(Path::new("script"), "#!/bin/bash\necho hi"),
            "bash"
        );
        assert_eq!(
            language_for_content(Path::new("script"), "#!/usr/bin/env node\nconsole.log(1)"),
            "javascript"
        );
        assert_eq!(
            language_for_content(Path::new("config"), "<?xml version=\"1.0\"?>\n<root/>"),
            "xml"
        );
        assert_eq!(
            language_for_content(Path::new("page"), "<!doctype html>\n<html></html>"),
            "html"
        );
        assert_eq!(
            language_for_content(Path::new("data"), "{\"a\": 1}"),
            "json"
        );
        assert_eq!(
            language_for_content(Path::new("LICENSE"), "plain text"),
            "text"
        );
    }
}
