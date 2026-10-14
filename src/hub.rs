//! Loopback client for ZECTRIX Hub.
//!
//! The private Hub repository could not be read from this environment.
//! This module follows the vocabulary app (`itopinion/zectrix-vocabulary`),
//! which is the public sibling that already speaks to Hub. The wire format
//! below is therefore an assumption, checked against that app and its
//! loopback test, not against `docs/RUNTIME_API.md`:
//!
//! - `GET  /runtime/status` → `{active, lease, device}`
//! - `GET  /runtime/events?after=` → `{events:[{lease, event}], cursor}`
//! - `POST /runtime/render` JSON `{layout, lease, present}` → PNG bytes
//! - `POST /runtime/audio` body `audio/wav`, headers `X-Lease` and `X-Volume`
//!   → JSON whose `result` is `Ok(Completed)` when playback finishes
//! - `POST /runtime/stop` JSON `{lease}`
//!
//! Authorization is `Authorization: Bearer $ZECTRIX_APP_TOKEN`. The token
//! never goes to the browser. `ZECTRIX_HUB_URL` must be `http://127.0.0.1`
//! or `http://localhost` with no userinfo, query, or path other than `/`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
pub struct Runtime {
    pub active: bool,
    pub lease: u64,
    #[serde(default)]
    pub device: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Event {
    pub lease: u64,
    pub event: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Events {
    pub events: Vec<Event>,
    pub cursor: u64,
}

pub struct Hub {
    client: reqwest::Client,
    url: String,
    token: String,
}

impl Hub {
    pub fn from_env() -> Result<Self> {
        let raw = std::env::var("ZECTRIX_HUB_URL")
            .context("缺少 ZECTRIX_HUB_URL，请从 ZECTRIX Hub 打开本应用")?;
        let url = reqwest::Url::parse(&raw)?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            bail!("ZECTRIX_HUB_URL 必须是本机 HTTP 根地址");
        }
        let token = std::env::var("ZECTRIX_APP_TOKEN").context("缺少 ZECTRIX_APP_TOKEN")?;
        if token.is_empty() {
            bail!("ZECTRIX_APP_TOKEN 为空");
        }
        Self::new(raw.trim_end_matches('/'), &token)
    }

    /// Settings page without a Hub. Render and status are local no-ops.
    pub fn dev_offline() -> Result<Self> {
        Self::new("", "")
    }

    fn new(url: &str, token: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(40))
            .build()?;
        Ok(Self {
            client,
            url: url.to_string(),
            token: token.to_string(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn is_offline(&self) -> bool {
        self.url.is_empty()
    }

    async fn checked(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let response = request.bearer_auth(&self.token).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!(
                "Hub 拒绝请求 ({status})：{}",
                text.chars().take(300).collect::<String>()
            );
        }
        Ok(response)
    }

    pub async fn status(&self) -> Result<Runtime> {
        if self.is_offline() {
            return Ok(Runtime {
                active: false,
                lease: 0,
                device: json!({"connected": false, "mode": "dev"}),
            });
        }
        Ok(self
            .checked(self.client.get(format!("{}/runtime/status", self.url)))
            .await?
            .json()
            .await?)
    }

    pub async fn events(&self, after: u64) -> Result<Events> {
        if self.is_offline() {
            tokio::time::sleep(Duration::from_secs(30)).await;
            return Ok(Events {
                events: Vec::new(),
                cursor: after,
            });
        }
        Ok(self
            .checked(
                self.client
                    .get(format!("{}/runtime/events", self.url))
                    .query(&[("after", after)])
                    .timeout(Duration::from_secs(8)),
            )
            .await?
            .json()
            .await?)
    }

    pub async fn render(&self, layout: Value, lease: u64, present: bool) -> Result<Vec<u8>> {
        if self.is_offline() {
            return Ok(PLACEHOLDER_PNG.to_vec());
        }
        Ok(self
            .checked(
                self.client
                    .post(format!("{}/runtime/render", self.url))
                    .json(&json!({"layout": layout, "lease": lease, "present": present})),
            )
            .await?
            .bytes()
            .await?
            .to_vec())
    }

    pub async fn audio(&self, wav: Vec<u8>, volume: u8, lease: u64) -> Result<Value> {
        if self.is_offline() {
            bail!("开发模式没有设备音频");
        }
        let result: Value = self
            .checked(
                self.client
                    .post(format!("{}/runtime/audio", self.url))
                    .timeout(Duration::from_secs(180))
                    .header("Content-Type", "audio/wav")
                    .header("X-Lease", lease.to_string())
                    .header("X-Volume", volume.to_string())
                    .body(wav),
            )
            .await?
            .json()
            .await?;
        if result["result"] != "Ok(Completed)" {
            bail!("设备音频未完成：{result}");
        }
        Ok(result)
    }

    pub async fn stop(&self, lease: u64) -> Result<()> {
        if self.is_offline() {
            return Ok(());
        }
        self.checked(
            self.client
                .post(format!("{}/runtime/stop", self.url))
                .json(&json!({"lease": lease})),
        )
        .await?;
        Ok(())
    }
}

/// 1×1 PNG so the settings page has a valid image before Hub answers.
const PLACEHOLDER_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 8, 215, 99, 248, 255, 255, 63, 0, 5, 254,
    2, 254, 167, 53, 129, 132, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
