use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::state::AppState;

struct Fixture {
    _temp: tempfile::TempDir,
    agent_dir: PathBuf,
    ui_dir: PathBuf,
    session_id: String,
    cwd: PathBuf,
}

fn write_fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let agent_dir = temp.path().join("agent");
    let ui_dir = temp.path().join("ui");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&ui_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        ui_dir.join("index.html"),
        "<!doctype html><title>pi-star test</title>",
    )
    .unwrap();
    std::fs::write(cwd.join("hello.txt"), "hello from the fixture\n").unwrap();

    let session_id = "test-session-0001";
    let encoded = pi_star_core::config::encode_cwd(&cwd);
    let sessions = agent_dir.join("sessions").join(encoded);
    std::fs::create_dir_all(&sessions).unwrap();
    let path = sessions.join(format!("2026-01-01T00-00-00.000Z_{session_id}.jsonl"));
    let now = "2026-01-01T00:00:00.000Z";
    let lines = [
        json!({"type":"session","version":3,"id":session_id,"timestamp":now,"cwd":cwd.to_string_lossy()}),
        json!({"type":"message","id":"u1","parentId":null,"timestamp":now,"message":{"role":"user","content":[{"type":"text","text":"hello fixture"}]}}),
        json!({"type":"message","id":"a1","parentId":"u1","timestamp":now,"message":{"role":"assistant","content":[{"type":"text","text":"fixture reply"}],"provider":"test","model":"test","usage":{"input":10,"output":2,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}}}}),
        json!({"type":"message","id":"t1","parentId":"a1","timestamp":now,"message":{"role":"toolResult","toolCallId":"call-1","toolName":"read","content":[{"type":"text","text":"file body"}],"isError":false}}),
        json!({"type":"session_info","id":"s1","parentId":"t1","timestamp":now,"name":"Fixture session"}),
    ];
    std::fs::write(
        &path,
        lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    Fixture {
        _temp: temp,
        agent_dir,
        ui_dir,
        session_id: session_id.to_string(),
        cwd,
    }
}

async fn router(fixture: &Fixture) -> axum::Router {
    router_with_password(fixture, None).await
}

async fn router_with_password(fixture: &Fixture, password: Option<&str>) -> axum::Router {
    let state = AppState::new_with_password(
        Some(fixture.agent_dir.clone()),
        fixture.ui_dir.clone(),
        "127.0.0.1",
        password.map(str::to_owned),
    )
    .await
    .unwrap();
    crate::app::router(state)
}

async fn call(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn call_text(app: axum::Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn health_and_static_spa() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "pi-star");

    let (status, text) = call_text(
        app.clone(),
        Request::builder()
            .uri("/login")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("pi-star test"));

    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/missing")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "Not found");
}

#[tokio::test]
async fn sessions_list_read_and_pagination() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/sessions")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], fixture.session_id);
    assert_eq!(sessions[0]["messageCount"], 3);
    assert_eq!(sessions[0]["firstMessage"], "hello fixture");
    assert_eq!(sessions[0]["name"], "Fixture session");

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/sessions/{}?tail=2", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["info"]["name"], "Fixture session");
    assert_eq!(body["context"]["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["context"]["thinkingLevel"], "off");
    assert!(body["tree"].as_array().is_some());

    let (status, body) = call(
        app,
        Request::builder()
            .uri(format!(
                "/api/sessions/{}/context?tail=1",
                fixture.session_id
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["context"]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(body["context"]["hasMore"], true);
}

#[tokio::test]
async fn session_ui_state_round_trip() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/sessions/ui-state")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"action":"set","ids":[fixture.session_id],"pinned":true}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["revision"], 1);
    assert!(body["state"]["sessions"][&fixture.session_id]["pinnedAt"].is_number());

    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/sessions/ui-state")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["revision"], 1);
}

#[tokio::test]
async fn files_list_read_and_index() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let path = fixture.cwd.to_string_lossy();
    let encoded = path.trim_start_matches('/');

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/files/{encoded}?type=list"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "hello.txt")
    );

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/files/{encoded}/hello.txt?type=read"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["content"]
            .as_str()
            .unwrap()
            .contains("hello from the fixture")
    );

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/files/{encoded}/hello.txt?type=meta"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["size"], 23);

    let boundary = "----pi-star-test-boundary";
    let upload_body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"upload.txt\"\r\nContent-Type: text/plain\r\n\r\nuploaded from test\n\r\n--{boundary}--\r\n"
    );
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/files/{encoded}?type=upload"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(upload_body))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["uploaded"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(fixture.cwd.join("upload.txt")).unwrap(),
        "uploaded from test\n"
    );

    let (status, body) = call(
        app,
        Request::builder()
            .uri(format!(
                "/api/file-index?cwd={}&q=hello",
                urlencoding(&fixture.cwd)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["matches"].as_array().unwrap()[0]["path"], "hello.txt");
}

#[tokio::test]
async fn agent_ensure_session_creates_a_file() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/agent/new")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "cwd": fixture.cwd.to_string_lossy(),
                    "type": "ensure_session"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = body["sessionId"].as_str().unwrap();
    assert!(
        pi_star_core::session::resolve_session_path(
            &pi_star_core::Config::new(
                Some(fixture.agent_dir.clone()),
                Some(fixture.ui_dir.clone())
            )
            .unwrap(),
            id
        )
        .is_some()
    );
}

#[tokio::test]
async fn mcp_test_reports_missing_server() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/mcp/test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"scope":"global","name":"missing"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn forbidden_path_is_rejected() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/files/%2Fetc/passwd?type=read")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "Access denied");
}

fn urlencoding(path: &Path) -> String {
    path.to_string_lossy()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[tokio::test]
async fn git_status_lists_untracked_files() {
    let fixture = write_fixture();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&fixture.cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["add", "hello.txt"]);
    git(&["commit", "-qm", "initial"]);
    std::fs::write(fixture.cwd.join("changed.txt"), "new file\n").unwrap();

    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/git/status?cwd={}", urlencoding(&fixture.cwd)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isGitRepository"], true);
    assert!(body["files"].as_array().unwrap().iter().any(|file| {
        file["filePath"].as_str().unwrap().ends_with("changed.txt") && file["status"] == "untracked"
    }));

    let changed = fixture.cwd.join("changed.txt");
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!(
                "/api/git/diff?cwd={}&path={}",
                urlencoding(&fixture.cwd),
                urlencoding(&changed)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["supported"], true);
    assert_eq!(body["status"], "untracked");
    assert!(body["patch"].as_str().unwrap().contains("+new file"));

    let (status, body) = call(
        app,
        Request::builder()
            .uri(format!("/api/worktrees?cwd={}", urlencoding(&fixture.cwd)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isGit"], true);
    assert_eq!(
        body["currentWorktreePath"],
        fixture.cwd.to_string_lossy().as_ref()
    );
    assert_eq!(body["worktrees"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn password_login_throttles_failures() {
    let fixture = write_fixture();
    let app = router_with_password(&fixture, Some("correct horse")).await;

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/web-auth")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"password":"wrong"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/web-auth")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"password":"wrong again"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body["retryAfterMs"].as_u64().unwrap() >= 1000);

    let (status, _) = call(
        app,
        Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn push_config_and_subscription_round_trip() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/push/config")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let public_key = body["publicKey"].as_str().unwrap().to_string();
    assert_eq!(public_key.len() > 80, true);

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/push/subscribe")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({
                "subscription": {
                    "endpoint": "https://example.com/push",
                    "keys": {
                        "p256dh": "BNDyYhDuDZJ1unUIFxBAM4UggPNXfTQ10VPlJdJ8Vx9psbztEuRj8L_iTuQr3pXbVhct3UVHo_yX1kmvZdpuCzE",
                        "auth": "46yScNguYqUuUyzJTB_CCw"
                    },
                    "locale": "en"
                }
            }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = call(
        app,
        Request::builder()
            .uri("/api/push/config")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(body["publicKey"], public_key);
}

#[tokio::test]
async fn real_pi_session_fixture_reads_and_forks() {
    let temp = tempfile::tempdir().unwrap();
    let agent_dir = temp.path().join("agent");
    let ui_dir = temp.path().join("ui");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&ui_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        ui_dir.join("index.html"),
        "<!doctype html><title>fixture</title>",
    )
    .unwrap();
    let fixture = include_str!("../../pi-star-core/tests/fixtures/session-with-tool.jsonl")
        .replace(
            "/home/ubuntu/workspace/pi-star/pi-star",
            &cwd.to_string_lossy(),
        );
    let encoded = pi_star_core::config::encode_cwd(&cwd);
    let dir = agent_dir.join("sessions").join(encoded);
    std::fs::create_dir_all(&dir).unwrap();
    let id = "d8612e2c-78c3-4929-90e9-d6b1777b46d6";
    std::fs::write(
        dir.join(format!("2026-01-01T00-00-00.000Z_{id}.jsonl")),
        fixture,
    )
    .unwrap();
    let fixture = Fixture {
        _temp: temp,
        agent_dir: agent_dir.clone(),
        ui_dir: ui_dir.clone(),
        session_id: id.to_string(),
        cwd: cwd.clone(),
    };
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/sessions/{id}?tail=10"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["info"]["id"], id);
    assert_eq!(body["stats"]["toolCalls"], 1);
    assert!(body["context"]["messages"].as_array().unwrap().len() >= 4);

    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/sessions/{id}/fork"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let fork_id = body["sessionId"].as_str().unwrap();
    assert_ne!(fork_id, id);
    assert_eq!(body["session"]["relation"]["kind"], "fork");
    assert_eq!(body["session"]["relation"]["originSessionId"], id);
}

#[tokio::test]
async fn oauth_login_emits_an_authorization_url() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/login/anthropic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let frame = body.frame().await.unwrap().unwrap();
    let data = frame.into_data().unwrap_or_default();
    let text = String::from_utf8_lossy(&data);
    assert!(text.contains("\"type\":\"auth\""));
    assert!(text.contains("claude.ai/oauth/authorize"));

    let (status, _) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login/anthropic")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"token":"missing","code":"x"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn skills_install_and_update_from_a_git_source() {
    let fixture = write_fixture();
    let repo = fixture.cwd.join("skill-repo");
    std::fs::create_dir_all(repo.join("skills/demo")).unwrap();
    std::fs::write(
        repo.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "skill"]);
    let package = format!("file://{}@demo", repo.to_string_lossy());
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/skills/install")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "package": package,
                    "scope": "project",
                    "cwd": fixture.cwd.to_string_lossy(),
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(fixture.cwd.join(".agents/skills/demo/SKILL.md").exists());
    assert!(fixture.cwd.join("skills-lock.json").exists());

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/skills?cwd={}", urlencoding(&fixture.cwd)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let skill = body["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|skill| skill["name"] == "demo")
        .expect("installed skill");
    assert!(skill["install"]["versionHash"].as_str().is_some());

    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/skills/update")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "cwd": fixture.cwd.to_string_lossy(),
                    "package": package,
                    "scope": "project",
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
}

#[tokio::test]
async fn plugins_install_update_and_remove_a_local_package() {
    let fixture = write_fixture();
    let plugin = fixture.cwd.join("my-plugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("index.js"), "export default {};\n").unwrap();
    let source = plugin.to_string_lossy().into_owned();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/plugins")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "action": "install",
                    "source": source,
                    "scope": "project",
                    "cwd": fixture.cwd.to_string_lossy(),
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(fixture.cwd.join("git/local/my-plugin/index.js").exists());
    assert!(fixture.cwd.join(".pi/settings.json").exists());

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/plugins?cwd={}", urlencoding(&fixture.cwd)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|package| package["source"] == source)
    );

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/plugins")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "action": "update",
                    "source": source,
                    "scope": "project",
                    "cwd": fixture.cwd.to_string_lossy(),
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/plugins")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "action": "remove",
                    "source": source,
                    "scope": "project",
                    "cwd": fixture.cwd.to_string_lossy(),
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!fixture.cwd.join("git/local/my-plugin").exists());
    assert!(body["packages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn mcp_oauth_discovers_registers_and_stores_tokens() {
    use axum::routing::{get, post};

    // The discovery payload must point at this listener, so build it after bind.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let oauth = axum::Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(move || async move {
                axum::Json(json!({
                    "authorization_endpoint": format!("http://127.0.0.1:{port}/authorize"),
                    "token_endpoint": format!("http://127.0.0.1:{port}/token"),
                    "registration_endpoint": format!("http://127.0.0.1:{port}/register"),
                    "scopes_supported": ["mcp"],
                }))
            }),
        )
        .route(
            "/register",
            post(|| async {
                axum::Json(json!({"client_id": "client-1", "client_secret": "secret-1"}))
            }),
        )
        .route(
            "/token",
            post(|| async {
                axum::Json(json!({
                    "access_token": "access-1",
                    "refresh_token": "refresh-1",
                    "token_type": "Bearer",
                    "expires_in": 3600
                }))
            }),
        );
    tokio::spawn(async move {
        let _ = axum::serve(listener, oauth).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let fixture = write_fixture();
    std::fs::write(
        fixture.agent_dir.join("mcp.json"),
        json!({"mcpServers": {"demo": {"url": format!("http://127.0.0.1:{port}/mcp")}}})
            .to_string(),
    )
    .unwrap();
    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/mcp/sign-in")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"scope": "global", "name": "demo"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["phase"], "authorize");
    let flow_id = body["flowId"].as_str().unwrap();
    let auth_url = reqwest::Url::parse(body["authorizationUrl"].as_str().unwrap()).unwrap();
    let state = auth_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .unwrap();

    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/mcp/sign-in/{flow_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"redirectUrl": format!("http://localhost/callback?code=abc&state={state}")})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["phase"], "done");
    let auth: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.agent_dir.join("mcp-auth.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        auth["mcp__demo|http://127.0.0.1:".to_string() + &port.to_string() + "/mcp"]["tokens"]["access_token"],
        "access-1"
    );
}

#[tokio::test]
async fn debug_snapshot_has_the_shape_the_panel_renders() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/agent/debug")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["now"].is_number(),
        "now is epoch ms, not a timestamp string"
    );
    let server = &body["server"];
    assert!(server["startedAt"].is_number());
    assert!(server["idleTimeoutMs"].is_number());
    assert!(server["heartbeatMs"].is_number());
    assert!(server["wrappers"]["alive"].is_number());
    assert!(server["eventStreams"]["total"].is_number());
    assert!(server["sseTotal"].is_number());
    assert!(server["pages"].is_array());
    assert!(server["fileWatches"].is_number());
    assert!(server["terminals"]["count"].is_number());
    assert!(body["sessions"].is_array());
    assert!(body["session"].is_null());

    let (status, detail) = call(
        app,
        Request::builder()
            .uri(format!("/api/agent/debug?session={}", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session = &detail["session"];
    assert_eq!(session["sessionId"], fixture.session_id);
    assert!(session["eventStreams"].is_array());
    assert!(session["subagents"]["runs"].is_array());
    assert!(session["subagents"]["notifications"].is_array());
    assert!(session["providers"].is_array());
}

#[tokio::test]
async fn models_config_refuses_an_invalid_image_limit() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let body = json!({
        "providers": {
            "gateway": {
                "models": [{
                    "id": "vision",
                    "inputLimits": { "images": { "resize": { "maxBytes": 1.5 } } }
                }]
            }
        }
    });
    let (status, reply) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/models-config")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        reply["error"],
        "Image size limit of gateway/vision must be a positive whole number of bytes"
    );

    let good = json!({
        "providers": {
            "gateway": {
                "models": [{
                    "id": "vision",
                    "inputLimits": { "images": { "resize": { "maxBytes": 524288 } } }
                }]
            }
        }
    });
    let (status, reply) = call(
        app,
        Request::builder()
            .method("PUT")
            .uri("/api/models-config")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(good.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["success"], true);
}

#[tokio::test]
async fn manifest_is_json_and_skill_content_accepts_file_path() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/manifest.webmanifest")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Pi Web");
    assert_eq!(body["start_url"], "/");
    assert_eq!(body["icons"].as_array().unwrap().len(), 2);

    let skill_dir = fixture.agent_dir.join("skills").join("fixture");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let skill = skill_dir.join("SKILL.md");
    std::fs::write(&skill, "---\nname: Fixture\n---\n\n# Fixture skill\n").unwrap();
    let reference = skill_dir.join("reference.md");
    std::fs::write(&reference, "# Reference\n").unwrap();

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!(
                "/api/skills/content?cwd={}&filePath={}",
                urlencoding(&fixture.cwd),
                urlencoding(&skill)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["content"].as_str().unwrap().contains("Fixture skill"));

    let (status, body) = call(
        app,
        Request::builder()
            .uri(format!(
                "/api/skills/content?cwd={}&filePath={}&document=reference.md",
                urlencoding(&fixture.cwd),
                urlencoding(&skill)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["content"], "# Reference\n");
}

#[tokio::test]
async fn mcp_overview_has_the_codemode_shape_the_panel_reads() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/mcp")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mcp"]["available"], true);
    assert_eq!(body["codemode"]["sandbox"]["state"], "not-checked");
    assert!(body["codemode"]["preference"].is_string());
    assert!(body["codemode"]["mode"]["value"].is_string());
    assert_eq!(
        body["codemode"]["mode"]["settingsPath"],
        fixture
            .agent_dir
            .join("settings.json")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(body["codemode"]["inlineBudget"]["default"], 3000);
    assert_eq!(body["codemode"]["inlineBudget"]["max"], 1_000_000);
}

fn fixture_session_path(fixture: &Fixture) -> PathBuf {
    fixture
        .agent_dir
        .join("sessions")
        .join(pi_star_core::config::encode_cwd(&fixture.cwd))
        .join(format!(
            "2026-01-01T00-00-00.000Z_{}.jsonl",
            fixture.session_id
        ))
}

#[tokio::test]
async fn agent_bash_output_requires_a_referenced_temp_file() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let name = format!("pi-bash-{}.log", uuid::Uuid::new_v4().simple());
    let output = std::env::temp_dir().join(name);
    std::fs::write(&output, "full bash output\nsecond line\n").unwrap();

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(fixture_session_path(&fixture))
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({
            "type": "message", "id": "bash-1", "parentId": "a1", "timestamp": "2026-01-01T00:00:00.000Z",
            "message": {
                "role": "bashExecution", "command": "echo full", "output": "full",
                "fullOutputPath": output.to_string_lossy(), "exitCode": 0
            }
        })
    )
    .unwrap();

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!(
                "/api/agent/{}/bash-output?path={}",
                fixture.session_id,
                urlencoding(&output)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["data"]["output"]
            .as_str()
            .unwrap()
            .contains("second line")
    );

    let other = std::env::temp_dir().join(format!("pi-bash-{}.log", uuid::Uuid::new_v4().simple()));
    std::fs::write(&other, "not referenced\n").unwrap();
    let (status, _) = call(
        app,
        Request::builder()
            .uri(format!(
                "/api/agent/{}/bash-output?path={}",
                fixture.session_id,
                urlencoding(&other)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let _ = std::fs::remove_file(output);
    let _ = std::fs::remove_file(other);
}

#[tokio::test]
async fn skills_check_reports_an_unsupported_lock_entry() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let skill_dir = fixture.cwd.join(".pi").join("skills").join("check");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: check\ndescription: fixture\n---\n\n# Check\n",
    )
    .unwrap();
    std::fs::write(
        fixture.cwd.join("skills-lock.json"),
        json!({
            "version": 1,
            "skills": {
                "check": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "ref": "HEAD",
                    "computedHash": "abc123"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/skills/check")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "cwd": fixture.cwd,
                    "package": "owner/repo@check",
                    "scope": "project"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["updates"][0]["state"], "unsupported");
    assert_eq!(body["updates"][0]["package"], "owner/repo@check");

    let skill_file = skill_dir.join("SKILL.md");
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("PATCH")
            .uri("/api/skills")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"filePath": skill_file, "disableModelInvocation": true}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["results"][0]["filePath"],
        skill_file.to_string_lossy().as_ref()
    );
    assert!(
        std::fs::read_to_string(&skill_file)
            .unwrap()
            .contains("disable-model-invocation: true")
    );

    let (status, _) = call(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/skills/check")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"cwd": fixture.cwd, "package": "owner/repo@check"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn models_refresh_and_tools_settings_keep_their_contract() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/tools/settings")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"codemode": "always", "codemodeMode": "only", "codemodeInlineBudget": 1234})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["codemode"], "always");
    assert_eq!(body["codemodeMode"]["value"], "only");
    assert_eq!(body["codemodeInlineBudget"]["value"], 1234);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/cache-miss-notices")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"showCacheMissNotices": true}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["showCacheMissNotices"], true);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/subagents/settings")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"maxConcurrent": 3}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["maxConcurrent"], 3);

    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/models/enabled")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["providers"].is_array());
    assert!(body["availableTotal"].is_number());
}

#[tokio::test]
async fn session_rename_search_stats_export_and_delete() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/sessions/{}", fixture.session_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"name": "Renamed fixture"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/sessions/{}", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["info"]["name"], "Renamed fixture");

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/sessions/search?q=hello%20fixture")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["results"].as_array().unwrap().iter().any(|result| {
        result.pointer("/session/id").and_then(Value::as_str) == Some(fixture.session_id.as_str())
    }));

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/sessions/stats")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"ids": [fixture.session_id]}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["stats"][fixture.session_id.as_str()]["totalActiveMs"].is_number());

    // Pin the deterministic Rust fallback: this checkout may have the pi CLI
    // installed, whose export template is not part of the assertion.
    unsafe { std::env::set_var("PI_STAR_PI_CLI", "/nonexistent/pi-star-pi-export") };
    let (status, text) = call_text(
        app.clone(),
        Request::builder()
            .uri(format!("/api/sessions/{}/export", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    unsafe { std::env::remove_var("PI_STAR_PI_CLI") };
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("fixture reply"));

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/sessions/{}/auto-name", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body["title"].as_str().unwrap().is_empty());

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/sessions/{}", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        app,
        Request::builder()
            .uri(format!("/api/sessions/{}", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cwd_browse_create_and_misc_reports() {
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/home")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["home"].as_str().unwrap().starts_with('/'));

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!(
                "/api/cwd/browse?path={}",
                urlencoding(&fixture.cwd)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["path"], fixture.cwd.to_string_lossy().as_ref());
    assert!(body["directories"].is_array());

    let new_dir = format!("created-{}", uuid::Uuid::new_v4().simple());
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/cwd/browse")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"path": fixture.cwd, "name": new_dir}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(std::path::Path::new(body["path"].as_str().unwrap()).is_dir());
    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/cwd/browse")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"path": fixture.cwd, "name": new_dir}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/cwd/validate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"cwd": fixture.cwd}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cwd"], fixture.cwd.to_string_lossy().as_ref());
    assert!(body["projectKey"].is_string());

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/open-in-explorer")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["platform"].is_string());

    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/web-auth")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["authenticated"], true);
}

#[tokio::test]
async fn terminal_create_get_and_kill() {
    let fixture = write_fixture();
    let app = router(&fixture).await;
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/terminal")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"cwd": fixture.cwd, "cols": 80, "rows": 24}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 32);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/terminal/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cwd"], fixture.cwd.to_string_lossy().as_ref());

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/terminal/{id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"type": "resize", "cols": 100, "rows": 30}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/terminal/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        app,
        Request::builder()
            .uri(format!("/api/terminal/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_config_is_listed_and_auth_keys_round_trip() {
    let fixture = write_fixture();
    std::fs::write(
        fixture.agent_dir.join("mcp.json"),
        json!({
            "mcpServers": {
                "demo": {"command": "echo", "args": ["hello"], "env": {"TOKEN": "secret"}}
            }
        })
        .to_string(),
    )
    .unwrap();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!("/api/mcp?cwd={}", urlencoding(&fixture.cwd)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["servers"][0]["name"], "demo");
    assert_eq!(body["servers"][0]["scope"], "global");
    assert_eq!(body["servers"][0]["transport"], "stdio");
    assert_eq!(body["servers"][0]["envNames"][0], "TOKEN");
    assert!(
        body["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["exists"] == true)
    );

    std::fs::create_dir_all(fixture.cwd.join(".pi")).unwrap();
    std::fs::write(
        fixture.cwd.join(".pi").join("mcp.json"),
        json!({"mcpServers": {"project-demo": {"command": "echo"}}}).to_string(),
    )
    .unwrap();
    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri(format!(
                "/api/project-trust?cwd={}",
                urlencoding(&fixture.cwd)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["requiresTrust"], true);
    assert_eq!(body["trusted"], false);
    assert_eq!(body["mcpServers"][0]["name"], "project-demo");

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/project-trust")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"cwd": fixture.cwd, "trusted": true}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trusted"], true);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/auth/api-key/openai")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"apiKey": "sk-test-fixture"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/auth/providers")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["providers"].as_array().is_some());
    assert!(body["oauthProviders"].as_array().is_some());

    let (status, _) = call(
        app,
        Request::builder()
            .method("DELETE")
            .uri("/api/auth/api-key/openai")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let auth = std::fs::read_to_string(fixture.agent_dir.join("auth.json")).unwrap();
    assert!(!auth.contains("sk-test-fixture"));
}

fn catalog_fixture() -> Value {
    json!({
        "demo": {
            "name": "Demo",
            "api": "https://api.demo.test",
            "models": {
                "demo-large": {
                    "id": "demo-large", "name": "Demo Large", "reasoning": true,
                    "modalities": {"input": ["text", "image"]},
                    "limit": {"context": 128000, "output": 8192},
                    "cost": {"input": 1, "output": 2, "cache_read": 0.1, "cache_write": 0}
                }
            }
        }
    })
}

#[tokio::test]
async fn model_catalog_refresh_and_app_update_use_the_remote_feed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::Router::new()
        .route(
            "/api.json",
            axum::routing::get(|| async { axum::Json(catalog_fixture()) }),
        )
        .route(
            "/release",
            axum::routing::get(|| async {
                axum::Json(json!({
                    "version": "9.9.9",
                    "html_url": "https://example.test/releases/9.9.9"
                }))
            }),
        );
    let handle = tokio::spawn(async move {
        axum::serve(listener, server).await.unwrap();
    });

    // SAFETY: this test owns the only readers of these variables while it runs;
    // tests that read them only check their shapes.
    unsafe {
        std::env::set_var("PI_MODELS_DEV_URL", format!("http://{addr}/api.json"));
        std::env::set_var("PI_STAR_UPDATE_URL", format!("http://{addr}/release"));
    }
    let fixture = write_fixture();
    let app = router(&fixture).await;

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .uri("/api/models-config/catalog?q=demo-large&provider=demo&baseUrl=https%3A%2F%2Fapi.demo.test&limit=10")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["models"][0]["key"], "demo/demo-large");
    assert_eq!(body["recommendation"]["metadataMethod"], "provider");
    assert_eq!(body["recommendation"]["preset"]["name"], "Demo Large");
    assert_eq!(body["recommendation"]["price"]["status"], "reliable");

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/models/refresh")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"provider": "demo"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["completed"], true);
    assert_eq!(body["changed"], true);
    let store = std::fs::read_to_string(fixture.agent_dir.join("models-store.json")).unwrap();
    assert!(store.contains("demo-large"));
    assert!(store.contains("128000"));

    let (status, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/models/refresh")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"provider": "demo"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["completed"], true);
    assert_eq!(body["changed"], false);

    let (status, body) = call(
        app,
        Request::builder()
            .uri("/api/app-update")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["latestVersion"], "9.9.9");
    assert_eq!(body["updateAvailable"], true);
    assert_eq!(body["releaseUrl"], "https://example.test/releases/9.9.9");

    unsafe {
        std::env::remove_var("PI_MODELS_DEV_URL");
        std::env::remove_var("PI_STAR_UPDATE_URL");
    }
    handle.abort();
}

#[tokio::test]
async fn auto_name_uses_the_configured_model_and_reports_usage() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"Fixture Title\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3}}\n\n\
                 data: [DONE]\n\n",
            )
        }),
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, server).await.unwrap();
    });

    let fixture = write_fixture();
    std::fs::write(
        fixture.agent_dir.join("models.json"),
        json!({
            "providers": {
                "test": {
                    "api": "openai-completions",
                    "baseUrl": format!("http://{addr}/v1"),
                    "apiKey": "test-key",
                    "models": [{"id": "test", "name": "Test Model"}]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        fixture.agent_dir.join("settings.json"),
        json!({"defaultProvider": "test", "defaultModel": "test"}).to_string(),
    )
    .unwrap();
    let app = router(&fixture).await;
    let (status, body) = call(
        app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/sessions/{}/auto-name", fixture.session_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Fixture Title");
    assert_eq!(body["usage"]["input"], 12);
    assert_eq!(body["usage"]["output"], 3);
    handle.abort();
}

#[tokio::test]
async fn mcp_runtime_lists_and_calls_http_tools() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::Router::new().route(
        "/",
        axum::routing::post(|axum::Json(request): axum::Json<Value>| async move {
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let result = match request.get("method").and_then(Value::as_str) {
                Some("initialize") => json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": {"name": "demo", "version": "1"}
                }),
                Some("tools/list") => json!({
                    "tools": [{
                        "name": "echo",
                        "description": "Echo text",
                        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
                    }]
                }),
                Some("tools/call") => {
                    let text = request
                        .pointer("/params/arguments/text")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    {
                        let text = format!("echo:{text}");
                        json!({"content": [{"type": "text", "text": text}]})
                    }
                }
                _ => Value::Null,
            };
            axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
        }),
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, server).await.unwrap();
    });

    let fixture = write_fixture();
    std::fs::write(
        fixture.agent_dir.join("mcp.json"),
        json!({"mcpServers": {"demo": {"url": format!("http://{addr}/")}}}).to_string(),
    )
    .unwrap();
    let config = pi_star_core::Config::new(
        Some(fixture.agent_dir.clone()),
        Some(fixture.ui_dir.clone()),
    )
    .unwrap();
    let mut runtime = pi_star_core::mcp_runtime::McpRuntime::connect(&config, &fixture.cwd).await;
    assert_eq!(runtime.tools.len(), 1);
    assert_eq!(runtime.tools[0].name, "mcp__demo__echo");
    assert_eq!(runtime.tools[0].description, "Echo text");
    let output = runtime
        .call("mcp__demo__echo", json!({"text": "hello"}))
        .await
        .unwrap();
    assert_eq!(output, "echo:hello");
    handle.abort();
}

#[tokio::test]
async fn pi_rpc_backend_speaks_jsonl_and_streams_events() {
    let temp = tempfile::tempdir().unwrap();
    let agent_dir = temp.path().join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let script = temp.path().join("fake-pi-rpc.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import sys, json, os, socket
def control_response(request):
    address = os.environ.get("PI_STAR_CONTROL_ADDR", "")
    token = os.environ.get("PI_STAR_CONTROL_TOKEN", "")
    host, _, port = address.rpartition(":")
    sock = socket.create_connection((host, int(port)))
    sock.sendall((json.dumps({"type": "hello", "token": token}) + "\n").encode())
    data = [{"name": "read", "label": "Read", "description": "Read a file", "parameters": {"type": "object"}, "active": True}]
    sock.sendall((json.dumps({"type": "response", "id": request["id"], "success": True, "data": data}) + "\n").encode())
    sock.close()
forked = False
for line in sys.stdin:
    try:
        command = json.loads(line)
    except Exception:
        continue
    kind = command.get("type")
    request_id = command.get("id")
    message = command.get("message") or ""
    if kind == "fork":
        forked = True
        print(json.dumps({"id": request_id, "type": "response", "command": "fork", "success": True, "data": {"cancelled": False}}), flush=True)
        continue
    if kind == "prompt" and message.startswith("/pi-star-host "):
        request = json.loads(message[len("/pi-star-host "):])
        control_response(request)
        print(json.dumps({"id": request_id, "type": "response", "command": "prompt", "success": True, "data": {"disposition": "handled"}}), flush=True)
        continue
    if kind == "get_state":
        print(json.dumps({"id": request_id, "type": "response", "command": "get_state", "success": True, "data": {
            "sessionId": "forked" if forked else "fake", "sessionFile": "/tmp/forked.jsonl" if forked else "", "model": {"provider": "fake", "id": "fake"},
            "thinkingLevel": "off", "isStreaming": False, "isCompacting": False,
            "autoCompactionEnabled": True, "messageCount": 0, "pendingMessageCount": 0
        }}), flush=True)
    elif kind == "prompt":
        print(json.dumps({"id": request_id, "type": "response", "command": "prompt", "success": True, "data": {"disposition": "started"}}), flush=True)
        print(json.dumps({"type": "agent_start"}), flush=True)
        print(json.dumps({"type": "message_start", "message": {"role": "assistant", "content": []}}), flush=True)
        print(json.dumps({"type": "message_end", "message": {"role": "assistant", "content": [{"type": "text", "text": "hello"}]}}), flush=True)
        print(json.dumps({"type": "agent_end", "messages": [], "willRetry": False}), flush=True)
        print(json.dumps({"type": "agent_settled"}), flush=True)
    else:
        print(json.dumps({"id": request_id, "type": "response", "command": kind, "success": True, "data": {}}), flush=True)
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let extension = temp.path().join("fake-extension.ts");
    std::fs::write(&extension, "export default function () {}\n").unwrap();
    let config = pi_star_core::Config::new(Some(agent_dir), None).unwrap();
    unsafe { std::env::set_var("PI_STAR_AGENT_EXTENSION", &extension) };
    let registry = crate::pi_rpc::PiRpcRegistry::new_with_cli(config, script.to_string_lossy());
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let completed = completed.clone();
        registry.set_completion_callback(std::sync::Arc::new(move |_, _| {
            completed.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
    }
    let session = registry
        .create(&cwd, crate::pi_rpc::PiSpawnOptions::default())
        .await
        .unwrap();
    assert!(!session.is_running());
    let mut events = session.subscribe();
    let response = session
        .send(&json!({"type": "prompt", "message": "hi"}))
        .await
        .unwrap();
    assert_eq!(response["disposition"], "started");
    let mut saw_message_end = false;
    let mut saw_prompt_done = false;
    for _ in 0..20 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        match event.get("type").and_then(Value::as_str) {
            Some("message_end") => saw_message_end = true,
            Some("prompt_done") => {
                saw_prompt_done = true;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_message_end);
    assert!(saw_prompt_done);
    assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
    let tools = session.send(&json!({"type": "get_tools"})).await.unwrap();
    assert_eq!(tools[0]["name"], "read");
    assert_eq!(tools[0]["active"], true);
    let old_id = session.id();
    let fork = session
        .send(&json!({"type": "fork", "entryId": "entry-1"}))
        .await
        .unwrap();
    assert_eq!(fork["newSessionId"], "forked");
    assert!(registry.get(&old_id).is_none());
    assert!(registry.get("forked").is_some());
    unsafe { std::env::remove_var("PI_STAR_AGENT_EXTENSION") };
    registry.shutdown();
}
