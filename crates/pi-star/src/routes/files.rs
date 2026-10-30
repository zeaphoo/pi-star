use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{FromRequest, Multipart, Path as AxumPath, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use pi_star_core::files;

use crate::debug_streams::StreamKind;
use crate::routes::{bad_request, forbidden, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct FileQuery {
    #[serde(rename = "type")]
    kind: Option<String>,
    hidden: Option<String>,
    offset: Option<u64>,
    conflict: Option<String>,
    /// The page that opened a watch stream (`?holder=`, Debug panel only).
    holder: Option<String>,
}

pub async fn get_file(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Response {
    let file_path = api_path_to_path(&path);
    let kind = query.kind.as_deref().unwrap_or("list");
    if !state.roots.is_allowed(&file_path) {
        return forbidden("Access denied");
    }
    let exists = file_path.exists();
    if !exists && kind != "watch" {
        return not_found("Not found");
    }
    let authorization_path = if exists {
        file_path.clone()
    } else {
        file_path.parent().unwrap_or(&file_path).to_path_buf()
    };
    if !state.roots.is_allowed_existing(&authorization_path) {
        return forbidden("Access denied");
    }
    match kind {
        "list" => {
            let show_hidden = query.hidden.as_deref() == Some("1");
            match files::list_directory(&file_path, show_hidden) {
                Ok(list) => no_store(list),
                Err(error) => internal(error),
            }
        }
        "read" => {
            let mime = files::mime_for(&file_path);
            let binary = !mime.starts_with("text/")
                && !matches!(
                    mime.as_str(),
                    "application/json"
                        | "application/javascript"
                        | "application/xml"
                        | "image/svg+xml"
                );
            if binary {
                return file_response(&file_path, &mime, &headers, false).await;
            }
            match files::read_text_chunk(&file_path, query.offset.unwrap_or(0)) {
                Ok(chunk) => no_store(chunk),
                Err(error) => internal(error),
            }
        }
        "download" => {
            let mime = files::mime_for(&file_path);
            file_response(&file_path, &mime, &headers, true).await
        }
        "meta" => match files::file_meta(&file_path) {
            Ok(meta) => no_store(meta),
            Err(error) => internal(error),
        },
        "preview" => {
            if file_path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
                return bad_request("Preview not available for this file type");
            }
            match files::docx_preview(&file_path) {
                Ok(html) => (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                        (header::CACHE_CONTROL, "no-cache".to_string()),
                        (
                            header::CONTENT_SECURITY_POLICY,
                            "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'".to_string(),
                        ),
                        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                    ],
                    html,
                )
                    .into_response(),
                Err(error) => internal(error),
            }
        }
        "watch" => watch_file(file_path, query, headers, state.debug_streams.clone()).await,
        _ => bad_request("Invalid file request type"),
    }
}

async fn file_response(path: &Path, mime: &str, headers: &HeaderMap, attachment: bool) -> Response {
    let Ok(metadata) = std::fs::metadata(path) else {
        return not_found("Not found");
    };
    if !metadata.is_file() {
        return bad_request("Not a file");
    }
    let size = metadata.len();
    let mut start = 0u64;
    let mut end = size.saturating_sub(1);
    let mut status = StatusCode::OK;
    if let Some(range) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(range) = range.strip_prefix("bytes=") {
            if let Some((from, to)) = range.split_once('-') {
                if let Ok(from) = from.parse::<u64>() {
                    start = from;
                    if let Ok(to) = to.parse::<u64>() {
                        end = to.min(size.saturating_sub(1));
                    }
                    status = StatusCode::PARTIAL_CONTENT;
                }
            }
        }
    }
    if start > end || start >= size {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{size}"))],
        )
            .into_response();
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return internal(error),
    };
    let mut file = tokio::fs::File::from_std(file);
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return internal("could not seek file");
    }
    let reader = file.take(end - start + 1);
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(reader));
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, end - start + 1);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    if attachment {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file")
            ),
        );
    }
    builder.body(body).unwrap_or_else(|error| internal(error))
}

async fn watch_file(
    path: PathBuf,
    query: FileQuery,
    headers: HeaderMap,
    streams: Arc<crate::debug_streams::DebugStreams>,
) -> Response {
    let stream_handle = streams.register(StreamKind::FileWatch, None, query.holder, &headers);
    let stream = async_stream::stream! {
        let mut last = files::modified_version(&path);
        stream_handle.connected();
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().event("connected").data(json!({"filePath": path.to_string_lossy()}).to_string())
        );
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let next = files::modified_version(&path);
            if next != last {
                last = next.clone();
                let payload = match next {
                    Some((size, mtime)) => json!({"size": size, "mtime": mtime}),
                    None => json!({"size": 0, "mtime": chrono::Utc::now().to_rfc3339()}),
                };
                stream_handle.write();
                yield Ok(axum::response::sse::Event::default().event("change").data(payload.to_string()));
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

pub async fn post_file(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<FileQuery>,
    request: Request,
) -> Response {
    let directory = api_path_to_path(&path);
    if !state.roots.is_allowed(&directory) {
        return forbidden("Access denied");
    }
    let kind = query.kind.as_deref().unwrap_or("upload");
    if kind == "allow-link" {
        let body = match Json::<AllowLinkBody>::from_request(request, &state).await {
            Ok(body) => body.0,
            Err(error) => return bad_request(error.to_string()),
        };
        if !directory.exists() {
            return not_found("Link not found");
        }
        let target = PathBuf::from(&body.target);
        if !target.is_dir() {
            return bad_request("target must be an existing directory");
        }
        state.roots.add(&target);
        return no_store(json!({ "path": target.to_string_lossy() }));
    }
    let Ok(metadata) = std::fs::metadata(&directory) else {
        return not_found("Upload directory not found");
    };
    if !metadata.is_dir() {
        return bad_request("Upload target is not a directory");
    }
    if !state.roots.is_allowed_existing(&directory) {
        return forbidden("Access denied");
    }
    if kind == "upload-check" {
        #[derive(Deserialize)]
        struct CheckBody {
            #[serde(rename = "fileNames")]
            file_names: Vec<String>,
        }
        let body = match Json::<CheckBody>::from_request(request, &state).await {
            Ok(body) => body.0,
            Err(error) => return bad_request(error.to_string()),
        };
        let (conflicts, non_replaceable) =
            files::inspect_upload_targets(&directory, &body.file_names);
        return no_store(json!({"conflicts": conflicts, "nonReplaceable": non_replaceable}));
    }
    let mut multipart = match Multipart::from_request(request, &state).await {
        Ok(multipart) => multipart,
        Err(error) => return bad_request(error.to_string()),
    };
    let strategy = query.conflict.as_deref().unwrap_or("error");
    let mut uploaded = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();
    let mut total = 0usize;
    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(file_name) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(data) => data,
            Err(error) => {
                errors.push(json!({"name": file_name, "error": error.to_string()}));
                continue;
            }
        };
        if data.len() > files::MAX_UPLOAD_FILE_BYTES {
            errors.push(json!({"name": file_name, "error": "Each upload must be 25MB or smaller"}));
            continue;
        }
        total += data.len();
        if total > files::MAX_UPLOAD_TOTAL_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"error": "Uploads must total 100MB or less"})),
            )
                .into_response();
        }
        let destination = directory.join(&file_name);
        let exists = destination.exists();
        if exists && strategy == "skip" {
            skipped.push(file_name);
            continue;
        }
        if exists && strategy == "error" {
            errors.push(json!({"name": file_name, "error": "File already exists"}));
            continue;
        }
        match files::write_upload(&directory, &file_name, &data, exists) {
            Ok(path) => uploaded.push(path.to_string_lossy().into_owned()),
            Err(error) => errors.push(json!({"name": file_name, "error": error.to_string()})),
        }
    }
    no_store(json!({"uploaded": uploaded, "skipped": skipped, "errors": errors}))
}

#[derive(Debug, Deserialize)]
pub struct AllowLinkBody {
    target: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct IndexQuery {
    cwd: Option<String>,
    q: Option<String>,
}

pub async fn file_index(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IndexQuery>,
) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() {
        return bad_request("cwd must be an absolute path");
    }
    if !state.roots.is_allowed(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    let query_text = query.q.unwrap_or_default();
    match files::index_files(&cwd, &query_text) {
        Ok((files, truncated)) if query_text.is_empty() => {
            no_store(json!({ "files": files, "truncated": truncated }))
        }
        Ok((files, _)) => no_store(json!({
            "matches": files.into_iter().map(|path| json!({"path": path, "isDir": false})).collect::<Vec<_>>()
        })),
        Err(error) => internal(error),
    }
}

fn api_path_to_path(path: &str) -> PathBuf {
    let path = path.trim_start_matches('/');
    if cfg!(windows) && path.len() >= 2 && path.as_bytes()[1] == b':' {
        PathBuf::from(path)
    } else {
        PathBuf::from(format!("/{path}"))
    }
}
