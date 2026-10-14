//! Packaged binary against a loopback Hub stub and a fixture feed.
//! No device, model, or live network.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct Stub {
    active: bool,
    lease: u64,
    events: Vec<Value>,
    frames: usize,
    last_layout: Value,
}

type SharedStub = Arc<Mutex<Stub>>;

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some("Bearer private-test-secret")
}

async fn status(
    State(stub): State<SharedStub>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let stub = stub.lock().await;
    Ok(Json(
        json!({"active": stub.active, "lease": stub.lease, "device": {"connected": false}}),
    ))
}

async fn events(
    State(stub): State<SharedStub>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    let stub = stub.lock().await;
    let after = query
        .get("after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    Ok(Json(json!({
        "events": stub.events.iter().filter(|event| event["seq"].as_u64().unwrap_or(0) > after).collect::<Vec<_>>(),
        "cursor": stub.events.len(),
    })))
}

async fn render(
    State(stub): State<SharedStub>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Vec<u8>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut stub = stub.lock().await;
    if body["present"] == true && (!stub.active || body["lease"] != stub.lease) {
        return Err(StatusCode::CONFLICT);
    }
    stub.frames += 1;
    stub.last_layout = body["layout"].clone();
    Ok(vec![137, 80, 78, 71, 13, 10, 26, 10])
}

async fn stop(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({"ok": true})))
}

async fn feed() -> axum::response::Response {
    let published = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S +0000")
        .to_string();
    let xml = format!(
        r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Fixture</title>
        <item><title>OpenAI releases a pocket model</title><link>https://example.com/pocket</link><pubDate>{published}</pubDate><description>First sentence about the model. Second sentence adds a fact. Third sentence says why it matters. Fourth is extra.</description></item>
        <item><title>OpenAI releases a pocket model for phones</title><link>https://example.com/verge</link><pubDate>{published}</pubDate><description>Same launch from another desk. Second sentence. Third sentence. Fourth.</description></item>
        <item><title>DeepMind predicts the weather</title><link>https://example.com/weather</link><pubDate>{published}</pubDate><description>A forecast model shipped. It covers more ground. It is not the pocket model. Done.</description></item>
        </channel></rss>"#
    );
    (
        [(axum::http::header::CONTENT_TYPE, "application/rss+xml")],
        xml,
    )
        .into_response()
}

use axum::response::IntoResponse;

async fn wait_status(
    client: &reqwest::Client,
    url: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    for _ in 0..200 {
        if let Ok(response) = client.get(format!("{url}/api/status")).send().await {
            if let Ok(value) = response.json::<Value>().await {
                if predicate(&value) {
                    return value;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("application did not reach the requested state");
}

#[tokio::test]
async fn refresh_fallback_and_device_keys_follow_the_lease() {
    let root = std::env::temp_dir().join(format!("zectrix-ai-daily-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let stub = Arc::new(Mutex::new(Stub::default()));
    let hub_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hub_url = format!("http://{}", hub_listener.local_addr().unwrap());
    let hub_routes = Router::new()
        .route("/runtime/status", get(status))
        .route("/runtime/events", get(events))
        .route("/runtime/render", post(render))
        .route("/runtime/stop", post(stop))
        .with_state(stub.clone());
    let hub_server =
        tokio::spawn(async move { axum::serve(hub_listener, hub_routes).await.unwrap() });

    let feed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let feed_addr = feed_listener.local_addr().unwrap();
    let feed_routes = Router::new().route("/news.xml", get(feed));
    let feed_server =
        tokio::spawn(async move { axum::serve(feed_listener, feed_routes).await.unwrap() });
    let catalog = format!(
        r#"
[[source]]
id = "fixture"
name = "Fixture"
name_zh = "夹具"
kind = "feed"
url = "http://{feed_addr}/news.xml"
weight = 1.2
enabled = true
window_hours = 48
max_items = 10
"#
    );
    std::fs::write(root.join("sources.toml"), catalog).unwrap();
    std::fs::write(
        root.join("settings.json"),
        r#"{"language":"en","refresh_hour":7,"refresh_minute":0,"item_count":10,"tts_enabled":false,"tts_speed":1.0,"volume":40,"ollama_url":"","ollama_model":"","voice_en":"af_heart","voice_zh":"zf_xiaobei","source_overrides":{},"device_date":""}"#,
    )
    .unwrap();

    let port_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_listener.local_addr().unwrap().port();
    drop(port_listener);
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for _ in 0..50 {
        if client
            .get(format!("{hub_url}/runtime/status"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_zectrix-ai-daily"))
        .env("ZECTRIX_HUB_URL", &hub_url)
        .env("ZECTRIX_APP_TOKEN", "private-test-secret")
        .env("ZECTRIX_APP_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("ZECTRIX_APP_DATA", &root)
        .env("ZECTRIX_APP_PORT", port.to_string())
        .env("ZECTRIX_DISABLE_SCHEDULER", "1")
        .env_remove("ZECTRIX_TTS_PYTHON")
        .env_remove("ZECTRIX_DEV")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut boot = None;
    for _ in 0..100 {
        if let Ok(Some(status)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                let _ = stderr.read_to_string(&mut err);
            }
            panic!("app exited {status}: {err}");
        }
        if let Ok(response) = client.get(format!("{url}/api/boot")).send().await {
            boot = Some(response.json::<Value>().await.unwrap());
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let app = Process(child);
    let boot = boot.expect("application did not open its settings page");
    assert!(!boot.to_string().contains("private-test-secret"));
    let token = boot["token"].as_str().unwrap();
    let denied = client
        .post(format!("{url}/api/refresh"))
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let blocked = client
        .post(format!("{url}/api/device/action"))
        .header("X-App-Token", token)
        .json(&json!({"action": "down"}))
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), 400);

    assert!(client
        .post(format!("{url}/api/refresh"))
        .header("X-App-Token", token)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let status = wait_status(&client, &url, |value| {
        value["digest"]["items"]
            .as_array()
            .map(|items| items.len() >= 2)
            .unwrap_or(false)
    })
    .await;
    assert_eq!(status["digest"]["backend"], "fallback");
    assert!(status["digest"]["items"].as_array().unwrap().len() <= 3);
    let titles: Vec<_> = status["digest"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_string())
        .collect();
    assert!(titles.iter().any(|title| title.contains("pocket")));
    assert!(titles
        .iter()
        .any(|title| title.contains("weather") || title.contains("DeepMind")));
    assert!(root.join("digests").read_dir().unwrap().next().is_some());

    {
        let mut stub = stub.lock().await;
        stub.active = true;
        stub.lease = 1;
    }
    wait_status(&client, &url, |value| value["runtime"]["active"] == true).await;
    stub.lock().await.events.push(
        json!({"seq": 1, "lease": 1, "event": {"type": "key", "key": "Down", "action": "Short"}}),
    );
    wait_status(&client, &url, |value| value["view"]["cursor"] == 1).await;
    stub.lock().await.events.push(json!({"seq": 2, "lease": 1, "event": {"type": "key", "key": "Confirm", "action": "Short"}}));
    wait_status(&client, &url, |value| value["view"]["mode"] == "detail").await;
    stub.lock().await.events.push(
        json!({"seq": 3, "lease": 1, "event": {"type": "key", "key": "Back", "action": "Short"}}),
    );
    wait_status(&client, &url, |value| {
        value["view"]["mode"] == "list" && value["view"]["cursor"] == 1
    })
    .await;
    stub.lock().await.events.push(
        json!({"seq": 4, "lease": 99, "event": {"type": "key", "key": "Down", "action": "Short"}}),
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    let stayed = client
        .get(format!("{url}/api/status"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(stayed["view"]["cursor"], 1);
    assert!(stub.lock().await.frames > 1);
    let layout = stub.lock().await.last_layout.to_string();
    assert!(layout.contains("canvas"));
    assert!(layout.contains("400"));

    drop(app);
    hub_server.abort();
    feed_server.abort();
    std::fs::remove_dir_all(root).unwrap();
}
