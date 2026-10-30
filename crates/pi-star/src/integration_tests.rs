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
        app,
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
