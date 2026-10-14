//! Loopback settings server and the device event loop.
//!
//! Binds `127.0.0.1` only. State-changing requests need the per-process
//! `X-App-Token` from `GET /api/boot`. That token is not the Hub credential.

use crate::config::{self, Settings};
use crate::device::{Board, Effect};
use crate::hub::{self, Hub};
use crate::layout::{self, detail_pages_for};
use crate::model::Digest;
use crate::pipeline;
use crate::speech::Speech;
use crate::store;
use crate::text::hex_prefix;
use anyhow::{bail, Context, Result};
use axum::extract::{Path, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::net::Ipv4Addr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex, Notify, RwLock};

pub(crate) struct Job {
    running: bool,
    last_error: String,
    last_finished: String,
}

pub struct App {
    pub settings: Mutex<Settings>,
    pub root: PathBuf,
    pub app_root: PathBuf,
    pub token: String,
    pub port: u16,
    pub dev: bool,
    pub hub: Hub,
    pub runtime: RwLock<hub::Runtime>,
    pub speech: Speech,
    pub tts_state: RwLock<String>,
    pub png: RwLock<Vec<u8>>,
    pub frame: AtomicU64,
    pub draw: watch::Sender<u64>,
    pub board: Mutex<Board>,
    pub digest: RwLock<Option<Digest>>,
    pub(crate) job: Mutex<Job>,
    pub audio_gate: Mutex<()>,
    pub briefing_gen: AtomicU64,
    pub reschedule: Notify,
}

pub type Shared = Arc<App>;

struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": self.0.to_string()})),
        )
            .into_response()
    }
}

type Api<T> = std::result::Result<Json<T>, ApiError>;

fn changed(app: &App) {
    app.draw.send_modify(|n| *n = n.wrapping_add(1));
}

pub async fn run() -> Result<()> {
    let dev = flag("ZECTRIX_DEV");
    let app_root = app_root(dev)?;
    let root = data_root(dev)?;
    std::fs::create_dir_all(root.join("digests"))?;
    std::fs::create_dir_all(root.join("audio"))?;
    std::fs::create_dir_all(root.join("logs"))?;
    let port = listen_port(dev)?;
    let hub = if dev && std::env::var("ZECTRIX_HUB_URL").is_err() {
        Hub::dev_offline()?
    } else {
        Hub::from_env()?
    };
    let settings = Settings::load(&root)?;
    let runtime = hub.status().await.context("无法连接 ZECTRIX Hub")?;
    let (draw, _) = watch::channel(0u64);
    let digest = load_visible(&root, &settings).await;
    let initial = hub
        .render(
            layout::card(
                &Board::default(),
                digest.as_ref(),
                false,
                &settings.language,
            ),
            runtime.lease,
            false,
        )
        .await?;
    let app = Arc::new(App {
        settings: Mutex::new(settings),
        speech: Speech::new(&app_root, &root),
        tts_state: RwLock::new("idle".into()),
        root,
        app_root,
        token: uuid::Uuid::new_v4().to_string(),
        port,
        dev,
        hub,
        runtime: RwLock::new(runtime),
        png: RwLock::new(initial),
        frame: AtomicU64::new(0),
        draw,
        board: Mutex::new(Board::default()),
        digest: RwLock::new(digest),
        job: Mutex::new(Job {
            running: false,
            last_error: String::new(),
            last_finished: String::new(),
        }),
        audio_gate: Mutex::new(()),
        briefing_gen: AtomicU64::new(1),
        reschedule: Notify::new(),
    });
    let router = Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/style.css", get(css))
        .route("/api/boot", get(boot))
        .route("/api/status", get(status))
        .route("/api/settings", post(save_settings))
        .route("/api/sources", get(list_sources))
        .route("/api/refresh", post(refresh_now))
        .route("/api/history", get(history))
        .route("/api/digest/{date}", get(read_digest))
        .route("/api/device/action", post(device_action))
        .route("/api/device/date", post(show_date))
        .route("/api/item/audio", post(preview_audio))
        .route("/audio/{key}", get(audio_file))
        .route("/screen.png", get(screen))
        .layer(middleware::from_fn_with_state(app.clone(), secure))
        .with_state(app.clone());
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    tokio::spawn(render_loop(app.clone()));
    tokio::spawn(consume(app.clone()));
    tokio::spawn(scheduler(app.clone()));
    println!("AI 日报已启动：http://127.0.0.1:{port}");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true")
    )
}

fn app_root(dev: bool) -> Result<PathBuf> {
    match std::env::var("ZECTRIX_APP_ROOT") {
        Ok(value) => Ok(PathBuf::from(value)),
        Err(_) if dev => Ok(std::env::current_dir()?),
        Err(_) => bail!("缺少 ZECTRIX_APP_ROOT，请从 ZECTRIX Hub 打开本应用"),
    }
}

fn data_root(dev: bool) -> Result<PathBuf> {
    match std::env::var("ZECTRIX_APP_DATA") {
        Ok(value) => Ok(PathBuf::from(value)),
        Err(_) if dev => Ok(PathBuf::from("dev-data")),
        Err(_) => bail!("缺少 ZECTRIX_APP_DATA，请从 ZECTRIX Hub 打开本应用"),
    }
}

fn listen_port(dev: bool) -> Result<u16> {
    for key in ["ZECTRIX_APP_PORT", "ZECTRIX_AI_DAILY_PORT"] {
        if let Ok(value) = std::env::var(key) {
            let port: u16 = value.parse().context("网页端口无效")?;
            if port == 0 {
                bail!("网页端口不能为 0");
            }
            return Ok(port);
        }
    }
    if dev {
        return Ok(43117);
    }
    bail!(
        "缺少 ZECTRIX_APP_PORT（也接受 ZECTRIX_AI_DAILY_PORT）。\
         词汇应用使用专属变量 ZECTRIX_VOCAB_PORT；本应用同时接受 Hub 通用端口名与专属名。"
    )
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn secure(State(app): State<Shared>, request: Request, next: Next) -> Response {
    let expected = format!("127.0.0.1:{}", app.port);
    let local = format!("localhost:{}", app.port);
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if host != expected && host != local {
        return (StatusCode::FORBIDDEN, "只允许本机访问").into_response();
    }
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if origin != format!("http://{expected}") && origin != format!("http://{local}") {
            return (StatusCode::FORBIDDEN, "拒绝跨站请求").into_response();
        }
    }
    if request.method() != Method::GET
        && request
            .headers()
            .get("x-app-token")
            .and_then(|value| value.to_str().ok())
            != Some(app.token.as_str())
    {
        return (StatusCode::FORBIDDEN, "请从本机应用页面操作").into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert(
        "content-security-policy",
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
            .parse()
            .unwrap(),
    );
    response
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}

async fn css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/style.css"),
    )
}

async fn boot(State(app): State<Shared>) -> Json<Value> {
    Json(json!({
        "token": app.token,
        "hub_url": app.hub.url(),
        "dev": app.dev,
    }))
}

async fn status(State(app): State<Shared>) -> Json<Value> {
    let settings = app.settings.lock().await.clone();
    let board = app.board.lock().await.clone();
    let digest = app.digest.read().await.clone();
    let job = {
        let job = app.job.lock().await;
        json!({"running": job.running, "last_error": job.last_error, "last_finished": job.last_finished})
    };
    let runtime = app.runtime.read().await.clone();
    let loading = job["running"] == true;
    let view = layout::view_model(&board, digest.as_ref(), loading, &settings.language);
    let public = digest.as_ref().map(Digest::public_view);
    let today = config::local_today(Local::now());
    let next = config::next_run(Local::now(), settings.refresh_hour, settings.refresh_minute);
    Json(json!({
        "settings": settings,
        "runtime": {"active": runtime.active, "lease": runtime.lease, "hub_url": app.hub.url(), "offline": app.hub.is_offline()},
        "device": runtime.device,
        "job": job,
        "view": view,
        "digest": public,
        "tts": app.tts_state.read().await.clone(),
        "speech_installed": app.speech.installed(),
        "revision": app.frame.load(Ordering::Relaxed),
        "today": today,
        "next_run": next.to_rfc3339(),
    }))
}

async fn save_settings(State(app): State<Shared>, Json(next): Json<Settings>) -> Api<Value> {
    next.validate()?;
    let previous = app.settings.lock().await.clone();
    let regenerate = previous.language != next.language
        || previous.item_count != next.item_count
        || previous.ollama_url != next.ollama_url
        || previous.ollama_model != next.ollama_model;
    let reschedule = previous.refresh_hour != next.refresh_hour
        || previous.refresh_minute != next.refresh_minute;
    let date_changed = previous.device_date != next.device_date;
    next.save(&app.root)?;
    *app.settings.lock().await = next.clone();
    if reschedule {
        app.reschedule.notify_waiters();
    }
    if date_changed {
        show_saved_date(&app, &next).await?;
    }
    let mut started = false;
    if regenerate {
        let date = visible_date(&next);
        if store::load_digest(&app.root, &date)?.is_some() {
            started = spawn_job(app.clone(), JobKind::Regenerate { date }).await?;
        }
    }
    changed(&app);
    Ok(Json(json!({"settings": next, "regenerating": started})))
}

async fn list_sources(State(app): State<Shared>) -> Api<Value> {
    let settings = app.settings.lock().await.clone();
    let sources = config::load_sources(&app.app_root, &app.root)?;
    let rows: Vec<_> = sources
        .iter()
        .map(|source| {
            json!({
                "id": source.id,
                "name": source.name,
                "name_zh": source.name_zh,
                "kind": source.kind,
                "url": source.url,
                "weight": source.weight,
                "window_hours": source.window_hours,
                "max_items": source.max_items,
                "enabled": settings.source_enabled(source),
            })
        })
        .collect();
    Ok(Json(json!({"sources": rows})))
}

async fn refresh_now(State(app): State<Shared>) -> Api<Value> {
    let date = config::local_today(Local::now());
    let refresh_date = date.clone();
    spawn_job(app, JobKind::Refresh { date }).await?;
    Ok(Json(json!({"accepted": true, "date": refresh_date})))
}

async fn history(State(app): State<Shared>) -> Api<Value> {
    let mut rows = Vec::new();
    for date in store::list_dates(&app.root)? {
        if let Some(digest) = store::load_digest(&app.root, &date)? {
            rows.push(json!({
                "date": digest.date,
                "language": digest.language,
                "count": digest.items.len(),
                "backend": digest.backend,
                "model": digest.model,
                "generated_at": digest.generated_at,
            }));
        }
    }
    Ok(Json(json!({"days": rows})))
}

async fn read_digest(State(app): State<Shared>, Path(date): Path<String>) -> Api<Value> {
    let digest = store::load_digest(&app.root, &date)?.context("这一天没有简报")?;
    Ok(Json(json!(digest.public_view())))
}

#[derive(Deserialize)]
struct DeviceAction {
    action: String,
    #[serde(default)]
    index: Option<usize>,
}

async fn device_action(State(app): State<Shared>, Json(body): Json<DeviceAction>) -> Api<Value> {
    let _lease = ensure_control(&app).await?;
    match body.action.as_str() {
        "up" => apply_key_owned(app.clone(), "Up", "Short").await?,
        "down" => apply_key_owned(app.clone(), "Down", "Short").await?,
        "select" => apply_key_owned(app.clone(), "Confirm", "Short").await?,
        "back" => apply_key_owned(app.clone(), "Back", "Short").await?,
        "briefing" => apply_key_owned(app.clone(), "Confirm", "Long").await?,
        "stop" => {
            halt_briefing(&app).await;
        }
        "open" => {
            let index = body.index.context("缺少条目编号")?;
            halt_briefing(&app).await;
            let digest = app.digest.read().await.clone();
            let count = digest.as_ref().map(|item| item.items.len()).unwrap_or(0);
            let mut board = app.board.lock().await;
            let effect = board.open(index, count);
            drop(board);
            changed(&app);
            if effect == Effect::Opened {
                maybe_speak_open(&app).await;
            }
        }
        _ => return Err(ApiError(anyhow::anyhow!("未知操作"))),
    }
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct DateBody {
    date: String,
}

async fn show_date(State(app): State<Shared>, Json(body): Json<DateBody>) -> Api<Value> {
    let mut settings = app.settings.lock().await.clone();
    settings.device_date = body.date;
    settings.validate()?;
    settings.save(&app.root)?;
    *app.settings.lock().await = settings.clone();
    show_saved_date(&app, &settings).await?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct PreviewBody {
    id: String,
}

async fn preview_audio(State(app): State<Shared>, Json(body): Json<PreviewBody>) -> Api<Value> {
    let digest = app
        .digest
        .read()
        .await
        .clone()
        .context("没有可朗读的简报")?;
    let item = digest
        .items
        .iter()
        .find(|item| item.id == body.id)
        .context("找不到这条简报")?;
    let path = synthesize(&app, &item.tts_script, &digest.language).await?;
    let key = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    Ok(Json(json!({"url": format!("/audio/{key}")})))
}

async fn audio_file(State(app): State<Shared>, Path(key): Path<String>) -> Response {
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(app.root.join(format!("audio/{key}.wav"))).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "audio/wav")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn screen(State(app): State<Shared>) -> Response {
    (
        [(header::CONTENT_TYPE, "image/png")],
        app.png.read().await.clone(),
    )
        .into_response()
}

enum JobKind {
    Refresh { date: String },
    Regenerate { date: String },
}

async fn spawn_job(app: Shared, kind: JobKind) -> Result<bool> {
    let mut job = app.job.lock().await;
    if job.running {
        bail!("正在整理简报，请稍等");
    }
    job.running = true;
    job.last_error.clear();
    drop(job);
    changed(&app);
    tokio::spawn(async move {
        let result = match &kind {
            JobKind::Refresh { date } => {
                let settings = app.settings.lock().await.clone();
                pipeline::run(&app.root, &app.app_root, &settings, date).await
            }
            JobKind::Regenerate { date } => {
                let settings = app.settings.lock().await.clone();
                pipeline::regenerate(&app.root, &settings, date).await
            }
        };
        match result {
            Ok(digest) => {
                publish(&app, digest).await;
                let mut job = app.job.lock().await;
                job.running = false;
                job.last_finished = Utc::now().to_rfc3339();
                job.last_error.clear();
            }
            Err(error) => {
                let mut job = app.job.lock().await;
                job.running = false;
                job.last_error = error.to_string();
                job.last_finished = Utc::now().to_rfc3339();
            }
        }
        changed(&app);
    });
    Ok(true)
}

async fn publish(app: &App, digest: Digest) {
    let settings = app.settings.lock().await.clone();
    if digest.date == visible_date(&settings) {
        *app.digest.write().await = Some(digest);
        app.board.lock().await.reset();
    }
}

fn visible_date(settings: &Settings) -> String {
    if settings.device_date.is_empty() {
        config::local_today(Local::now())
    } else {
        settings.device_date.clone()
    }
}

async fn load_visible(root: &FsPath, settings: &Settings) -> Option<Digest> {
    let date = visible_date(settings);
    store::load_digest(root, &date).ok().flatten()
}

async fn show_saved_date(app: &App, settings: &Settings) -> Result<()> {
    let date = visible_date(settings);
    let digest = store::load_digest(&app.root, &date)?;
    *app.digest.write().await = digest;
    app.board.lock().await.reset();
    changed(app);
    Ok(())
}

async fn ensure_control(app: &App) -> Result<u64> {
    if app.hub.is_offline() {
        return Ok(0);
    }
    let state = refresh_runtime(app).await?;
    if !state.active {
        bail!("请先在 ZECTRIX Hub 中打开 AI 日报");
    }
    Ok(state.lease)
}

async fn refresh_runtime(app: &App) -> Result<hub::Runtime> {
    let next = app.hub.status().await?;
    apply_runtime(app, next).await;
    Ok(app.runtime.read().await.clone())
}

async fn apply_runtime(app: &App, next: hub::Runtime) {
    let changed_lease = {
        let mut state = app.runtime.write().await;
        if !app.hub.is_offline() && next.lease < state.lease {
            return;
        }
        let changed = state.active != next.active || state.lease != next.lease;
        *state = next;
        changed
    };
    if changed_lease {
        app.briefing_gen.fetch_add(1, Ordering::Relaxed);
        let mut board = app.board.lock().await;
        board.briefing = false;
        board.stop_armed = false;
        drop(board);
        changed(app);
    }
}

async fn consume(app: Shared) {
    let mut cursor = 0u64;
    loop {
        let batch = match app.hub.events(cursor).await {
            Ok(batch) => batch,
            Err(error) => {
                let mut state = app.runtime.read().await.clone();
                state.active = false;
                if state.device.is_null() {
                    state.device = json!({});
                }
                state.device["connected"] = json!(false);
                state.device["error"] = json!(format!("Hub 连接中断：{error}"));
                apply_runtime(&app, state).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        if refresh_runtime(&app).await.is_err() {
            continue;
        }
        cursor = batch.cursor;
        let state = app.runtime.read().await.clone();
        for item in batch.events {
            if !state.active || item.lease != state.lease {
                continue;
            }
            let kind = item.event["type"].as_str().unwrap_or("");
            if kind == "key" {
                let key = item.event["key"].as_str().unwrap_or("");
                let action = item.event["action"].as_str().unwrap_or("Short");
                if let Err(error) = apply_key_owned(app.clone(), key, action).await {
                    app.board.lock().await.notice = error.to_string();
                    changed(&app);
                }
            } else if kind == "disconnected" {
                halt_briefing(&app).await;
            }
        }
    }
}

async fn apply_key_owned(app: Shared, key: &str, action: &str) -> Result<()> {
    let digest = app.digest.read().await.clone();
    let count = digest.as_ref().map(|item| item.items.len()).unwrap_or(0);
    let mut board = app.board.lock().await;
    let index = board
        .detail
        .as_ref()
        .map(|detail| detail.index)
        .unwrap_or(board.cursor);
    let pages = detail_pages_for(digest.as_ref(), index);
    let effect = board.handle(key, action, count, pages);
    drop(board);
    changed(&app);
    match effect {
        Effect::Opened => maybe_speak_open(&app).await,
        Effect::Speak => {
            if let Err(error) = speak_current(&app).await {
                app.board.lock().await.notice = error.to_string();
                changed(&app);
            }
        }
        Effect::StartBriefing => start_briefing(app).await,
        Effect::StopBriefing => halt_briefing(&app).await,
        Effect::None => {}
    }
    Ok(())
}

async fn start_briefing(app: Shared) {
    let enabled = app.settings.lock().await.tts_enabled;
    if !enabled {
        let english = app.settings.lock().await.language == "en";
        let mut board = app.board.lock().await;
        board.briefing = false;
        board.notice = if english {
            "Speech is off. Turn it on in settings.".into()
        } else {
            "朗读已关闭。可在网页设置里打开。".into()
        };
        drop(board);
        changed(&app);
        return;
    }
    let gen = app.briefing_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let start = app.board.lock().await.cursor;
    let player = app.clone();
    tokio::spawn(async move {
        if let Err(error) = briefing_loop(player.clone(), start, gen).await {
            let mut board = player.board.lock().await;
            board.notice = error.to_string();
            board.briefing = false;
            drop(board);
            changed(&player);
        }
    });
    let armer = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        armer.board.lock().await.arm_stop();
    });
}

async fn maybe_speak_open(app: &App) {
    if !app.settings.lock().await.tts_enabled {
        return;
    }
    if let Err(error) = speak_current(app).await {
        app.board.lock().await.notice = error.to_string();
        changed(app);
    }
}

async fn speak_current(app: &App) -> Result<()> {
    let digest = app.digest.read().await.clone().context("没有简报")?;
    let board = app.board.lock().await.clone();
    let index = board
        .detail
        .as_ref()
        .map(|detail| detail.index)
        .context("没有打开的条目")?;
    let item = digest.items.get(index).context("条目不存在")?;
    let script = item.tts_script.clone();
    let language = digest.language.clone();
    let path = synthesize(app, &script, &language).await?;
    play_file(app, &path).await
}

async fn briefing_loop(app: Shared, start: usize, gen: u64) -> Result<()> {
    let digest = app.digest.read().await.clone().context("没有简报")?;
    if digest.items.is_empty() {
        bail!("没有可播报的条目");
    }
    for index in start..digest.items.len() {
        if app.briefing_gen.load(Ordering::Relaxed) != gen {
            return Ok(());
        }
        {
            let mut board = app.board.lock().await;
            if !board.briefing {
                return Ok(());
            }
            board.cursor = index;
            board.detail = None;
            board.notice.clear();
        }
        changed(&app);
        let item = &digest.items[index];
        let path = synthesize(&app, &item.tts_script, &digest.language).await?;
        if app.briefing_gen.load(Ordering::Relaxed) != gen {
            return Ok(());
        }
        play_file(&app, &path).await?;
    }
    if app.briefing_gen.load(Ordering::Relaxed) == gen {
        let mut board = app.board.lock().await;
        board.briefing = false;
        board.notice.clear();
        drop(board);
        changed(&app);
    }
    Ok(())
}

async fn halt_briefing(app: &App) {
    app.briefing_gen.fetch_add(1, Ordering::Relaxed);
    app.board.lock().await.briefing = false;
    let lease = app.runtime.read().await.lease;
    let _ = app.hub.stop(lease).await;
    changed(app);
}

async fn synthesize(app: &App, text: &str, language: &str) -> Result<PathBuf> {
    let settings = app.settings.lock().await.clone();
    let voice = settings.voice_for(language).to_string();
    let speed = settings.tts_speed;
    let material = format!("kokoro|{language}|{voice}|{speed:.2}|{text}");
    let key = hex_prefix(Sha256::digest(material.as_bytes()).as_slice(), 32);
    let path = app.root.join(format!("audio/{key}.wav"));
    if tokio::fs::metadata(&path)
        .await
        .map(|meta| meta.len() > 44)
        .unwrap_or(false)
    {
        return Ok(path);
    }
    *app.tts_state.write().await = "loading".into();
    let result = app
        .speech
        .call(json!({
            "text": text.chars().take(1800).collect::<String>(),
            "path": path,
            "lang": language,
            "voice": voice,
            "speed": speed,
        }))
        .await;
    match result {
        Ok(_) => {
            *app.tts_state.write().await = "ready".into();
            Ok(path)
        }
        Err(error) => {
            *app.tts_state.write().await = format!("error: {error}");
            Err(error)
        }
    }
}

async fn play_file(app: &App, path: &FsPath) -> Result<()> {
    let _gate = app.audio_gate.lock().await;
    if app.hub.is_offline() {
        return Ok(());
    }
    let state = refresh_runtime(app).await?;
    if !state.active {
        bail!("应用已切到后台，已停止朗读");
    }
    if state.device["connected"] == false {
        return Ok(());
    }
    let bytes = tokio::fs::read(path).await?;
    let volume = app.settings.lock().await.volume;
    app.hub.audio(bytes, volume, state.lease).await?;
    Ok(())
}

async fn render_loop(app: Shared) {
    let mut rx = app.draw.subscribe();
    loop {
        if let Err(error) = render_once(&app).await {
            eprintln!("render: {error}");
            tokio::select! {
                result = rx.changed() => { if result.is_err() { break; } }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        } else if rx.changed().await.is_err() {
            break;
        }
    }
}

async fn render_once(app: &App) -> Result<()> {
    let settings = app.settings.lock().await.clone();
    let board = app.board.lock().await.clone();
    let digest = app.digest.read().await.clone();
    let loading = app.job.lock().await.running && digest.is_none();
    let runtime = app.runtime.read().await.clone();
    let png = app
        .hub
        .render(
            layout::card(&board, digest.as_ref(), loading, &settings.language),
            runtime.lease,
            runtime.active,
        )
        .await?;
    *app.png.write().await = png;
    app.frame.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

async fn scheduler(app: Shared) {
    if flag("ZECTRIX_DISABLE_SCHEDULER") {
        return;
    }
    catch_up(app.clone()).await;
    loop {
        let settings = app.settings.lock().await.clone();
        let next = config::next_run(Local::now(), settings.refresh_hour, settings.refresh_minute);
        let wait = (next - Local::now())
            .to_std()
            .unwrap_or(Duration::from_secs(30));
        tokio::select! {
            _ = tokio::time::sleep(wait) => {
                let date = config::local_today(Local::now());
                let _ = spawn_job(app.clone(), JobKind::Refresh { date }).await;
            }
            _ = app.reschedule.notified() => {}
        }
    }
}

async fn catch_up(app: Shared) {
    let settings = app.settings.lock().await.clone();
    let now = Local::now();
    let Some(naive) = now.date_naive().and_hms_opt(
        settings.refresh_hour.into(),
        settings.refresh_minute.into(),
        0,
    ) else {
        return;
    };
    let Some(slot) = naive
        .and_local_timezone(Local)
        .single()
        .or_else(|| naive.and_local_timezone(Local).earliest())
    else {
        return;
    };
    if now < slot {
        return;
    }
    let today = config::local_today(now);
    if store::load_digest(&app.root, &today)
        .ok()
        .flatten()
        .is_none()
    {
        let _ = spawn_job(app, JobKind::Refresh { date: today }).await;
    }
}
