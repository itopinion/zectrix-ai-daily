const i18n = {
  zh: {
    brand: "AI 日报",
    lang: "English",
    deviceOff: "未连接设备",
    deviceOn: "设备已连接",
    deviceDev: "开发模式，未接 Hub",
    kicker: "全球人工智能早报",
    pending: "今天的简报还在排版",
    refresh: "立即刷新",
    refreshing: "正在抓取信源并整理…",
    saved: "设定已保存",
    emptyTitle: "这一天还没有简报",
    emptyBody: "可以现在刷新，或等到设定的时间自动整理。可用 Codex 整理中文摘要，也可直接阅读信源原文。",
    device: "Note 4 画面",
    hint: "上下选择，短按确认进入详情并自动朗读（需开启朗读），再次短按确认返回并停止。连播请点网页按钮；长按是设备录音。",
    settings: "早报设定",
    tts: "打开朗读",
    modelHint: "Codex 使用本机已登录的账号整理新闻，不需要安装文本模型。换语言会用已保存的原文重新整理。",
    save: "保存设定",
    sources: "信源",
    saveSources: "保存信源开关",
    history: "往日简报",
    noHistory: "还没有往日归档。",
    onDevice: "在设备上显示",
    fallback: "未使用模型，按信源权重与时间排序",
    model: "AI 整理",
    items: "条",
    up: "上", down: "下", select: "确认", back: "返回", briefing: "连播", stop: "停止",
    open: "原文",
    listen: "试听",
  },
  en: {
    brand: "AI Daily",
    lang: "中文",
    deviceOff: "Device idle",
    deviceOn: "Device connected",
    deviceDev: "Dev mode, Hub offline",
    kicker: "A morning briefing on AI",
    pending: "Today's edition is not set yet",
    refresh: "Refresh now",
    refreshing: "Fetching sources and editing…",
    saved: "Settings saved",
    emptyTitle: "No briefing for this day",
    emptyBody: "Refresh now, or wait for the scheduled run. Choose Codex to edit the briefing, or read the original feeds.",
    device: "Note 4 preview",
    hint: "Up/Down select. Short confirm opens and reads when speech is on; confirm again returns and stops. Use the web Play button for continuous play. Holding is for device recording.",
    settings: "Edition settings",
    tts: "Read aloud",
    modelHint: "Codex uses your existing local sign-in; no local text model is needed. Switching language rewrites the saved stories.",
    save: "Save settings",
    sources: "Sources",
    saveSources: "Save source switches",
    history: "Archive",
    noHistory: "No archived days yet.",
    onDevice: "Show on device",
    fallback: "No model — ranked by source weight and time",
    model: "AI edited",
    items: "items",
    up: "Up", down: "Down", select: "OK", back: "Back", briefing: "Play", stop: "Stop",
    open: "Source",
    listen: "Listen",
  },
};

let token = "";
let language = "zh";
let settings = null;
let sources = [];
let viewingDate = "";

const $ = (id) => document.getElementById(id);

function t(key) {
  return (i18n[language] || i18n.zh)[key];
}

function applyChrome() {
  document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  document.title = `${t("brand")} · ZECTRIX`;
  $("brandName").textContent = t("brand");
  $("langToggle").textContent = t("lang");
  $("kicker").textContent = t("kicker");
  $("refresh").textContent = t("refresh");
  $("emptyTitle").textContent = t("emptyTitle");
  $("emptyBody").textContent = t("emptyBody");
  $("deviceHeading").textContent = t("device");
  $("keyHint").textContent = t("hint");
  $("settingsHeading").textContent = t("settings");
  $("ttsLabel").textContent = t("tts");
  $("modelHint").textContent = t("modelHint");
  $("saveSettings").textContent = t("save");
  $("sourcesHeading").textContent = t("sources");
  $("saveSources").textContent = t("saveSources");
  $("historyHeading").textContent = t("history");
  $("keyUp").textContent = t("up");
  $("keyDown").textContent = t("down");
  $("keySelect").textContent = t("select");
  $("keyBack").textContent = t("back");
  $("keyBrief").textContent = t("briefing");
  $("keyStop").textContent = t("stop");
}

async function api(path, data) {
  const headers = {};
  if (data !== undefined) {
    headers["Content-Type"] = "application/json";
    headers["X-App-Token"] = token;
  }
  const response = await fetch(path, {
    method: data !== undefined ? "POST" : "GET",
    headers,
    body: data !== undefined ? JSON.stringify(data) : undefined,
  });
  if (!response.ok) {
    let message = response.statusText;
    try {
      const body = await response.json();
      message = body.error || JSON.stringify(body);
    } catch {
      message = await response.text();
    }
    throw new Error(message);
  }
  const type = response.headers.get("content-type") || "";
  if (!type.includes("json")) return null;
  return response.json();
}

function toast(message) {
  const node = $("toast");
  node.hidden = false;
  node.textContent = message;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { node.hidden = true; }, 3200);
}

function safeUrl(value) {
  try {
    const url = new URL(value);
    if (url.protocol === "http:" || url.protocol === "https:") return url.href;
  } catch { /* ignore */ }
  return "";
}

function fillSettings(next) {
  settings = next;
  language = next.language === "en" ? "en" : "zh";
  applyChrome();
  $("refreshTime").value = `${String(next.refresh_hour).padStart(2, "0")}:${String(next.refresh_minute).padStart(2, "0")}`;
  $("itemCount").value = next.item_count;
  $("ttsEnabled").checked = next.tts_enabled;
  $("ttsSpeed").value = next.tts_speed;
  $("speedOut").textContent = Number(next.tts_speed).toFixed(2);
  $("volume").value = next.volume;
  $("volumeOut").textContent = next.volume;
  $("llmBackend").value = next.llm_backend || "ollama";
  $("ollamaFields").hidden = next.llm_backend !== "ollama";
  $("ollamaUrl").value = next.ollama_url;
  $("ollamaModel").value = next.ollama_model;
  $("voiceEn").value = next.voice_en;
  $("voiceZh").value = next.voice_zh;
}

function readSettings() {
  const [hour, minute] = $("refreshTime").value.split(":");
  const overrides = {};
  document.querySelectorAll("[data-source]").forEach((input) => {
    overrides[input.dataset.source] = input.checked;
  });
  return {
    ...settings,
    language,
    refresh_hour: Number(hour),
    refresh_minute: Number(minute),
    item_count: Number($("itemCount").value),
    tts_enabled: $("ttsEnabled").checked,
    tts_speed: Number($("ttsSpeed").value),
    volume: Number($("volume").value),
    llm_backend: $("llmBackend").value,
    ollama_url: $("ollamaUrl").value.trim(),
    ollama_model: $("ollamaModel").value.trim(),
    voice_en: $("voiceEn").value.trim(),
    voice_zh: $("voiceZh").value.trim(),
    source_overrides: Object.keys(overrides).length ? overrides : settings.source_overrides,
  };
}

function renderStories(digest) {
  const list = $("stories");
  list.replaceChildren();
  const empty = $("empty");
  if (!digest || !digest.items || digest.items.length === 0) {
    empty.hidden = false;
    $("editionTitle").textContent = t("pending");
    $("editionMeta").textContent = "";
    $("warnings").hidden = true;
    return;
  }
  empty.hidden = true;
  $("editionTitle").textContent = language === "zh"
    ? `${digest.date} 的人工智能早报`
    : `AI briefing · ${digest.date}`;
  const how = digest.backend === "fallback"
    ? t("fallback")
    : `${t("model")} · ${digest.model || digest.backend}`;
  $("editionMeta").textContent = `${how} · ${digest.items.length} ${t("items")} · ${digest.generated_at || ""}`;
  const warnings = $("warnings");
  if (digest.warnings && digest.warnings.length) {
    warnings.hidden = false;
    warnings.textContent = digest.warnings.join("\n");
  } else {
    warnings.hidden = true;
  }
  digest.items.forEach((item, index) => {
    const row = document.createElement("li");
    row.className = "story";
    const numeral = document.createElement("span");
    numeral.className = "index";
    numeral.textContent = String(index + 1).padStart(2, "0");
    const title = document.createElement("button");
    title.className = "title";
    title.type = "button";
    title.textContent = item.title;
    title.addEventListener("click", () => device({ action: "open", index }));
    const meta = document.createElement("p");
    meta.className = "meta";
    const link = safeUrl(item.url);
    meta.textContent = `${(item.sources || [item.source]).join(" · ")}`;
    if (link) {
      meta.append(" · ");
      const anchor = document.createElement("a");
      anchor.href = link;
      anchor.target = "_blank";
      anchor.rel = "noopener";
      anchor.textContent = t("open");
      meta.append(anchor);
    }
    const summary = document.createElement("p");
    summary.textContent = item.summary;
    row.append(numeral, title, meta, summary);
    list.append(row);
  });
}

function renderEink(view) {
  const screen = $("eink");
  screen.replaceChildren();
  const header = document.createElement("header");
  const mast = document.createElement("strong");
  mast.textContent = language === "en" ? "AI DAILY" : "AI 日报";
  const date = document.createElement("span");
  date.textContent = view && view.date ? view.date : "";
  header.append(mast, date);
  const body = document.createElement("div");
  body.className = "rows";
  if (!view || view.mode === "empty" || view.mode === "loading") {
    const line = document.createElement("p");
    line.textContent = view && view.footer ? view.footer : t("emptyTitle");
    body.append(line);
  } else if (view.mode === "detail" && view.detail) {
    const title = document.createElement("p");
    title.className = "detail-title";
    title.textContent = view.detail.title;
    const meta = document.createElement("p");
    meta.className = "detail-meta";
    meta.textContent = view.detail.meta;
    body.append(title, meta);
    (view.detail.lines || []).forEach((text) => {
      const line = document.createElement("p");
      line.className = "detail-line";
      line.textContent = text;
      body.append(line);
    });
  } else {
    (view.items || []).forEach((item) => {
      const row = document.createElement("div");
      row.className = item.selected ? "row selected" : "row";
      const title = document.createElement("div");
      title.textContent = `${item.speaking ? "● " : item.selected ? "▸ " : ""}${item.title}`;
      const meta = document.createElement("small");
      meta.textContent = item.source;
      row.append(title, meta);
      body.append(row);
    });
  }
  const footer = document.createElement("footer");
  footer.textContent = view && view.notice ? view.notice : (view && view.footer) || "";
  screen.append(header, body, footer);
}

function renderSources(rows) {
  sources = rows;
  const list = $("sources");
  list.replaceChildren();
  rows.forEach((source) => {
    const item = document.createElement("li");
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.dataset.source = source.id;
    input.checked = source.enabled;
    const name = document.createElement("span");
    name.textContent = language === "zh" && source.name_zh ? source.name_zh : source.name;
    label.append(input, name);
    const kind = document.createElement("small");
    kind.textContent = source.kind;
    item.append(label, kind);
    list.append(item);
  });
}

function renderHistory(days) {
  const list = $("history");
  list.replaceChildren();
  if (!days.length) {
    const item = document.createElement("li");
    item.textContent = t("noHistory");
    list.append(item);
    return;
  }
  days.forEach((day) => {
    const item = document.createElement("li");
    const open = document.createElement("button");
    open.type = "button";
    open.textContent = `${day.date} · ${day.count}`;
    if (day.date === viewingDate) open.className = "on";
    open.addEventListener("click", () => showArchive(day.date));
    const device = document.createElement("button");
    device.type = "button";
    device.textContent = t("onDevice");
    device.addEventListener("click", () => api("/api/device/date", { date: day.date }).then(refresh).catch((error) => toast(error.message)));
    item.append(open, device);
    list.append(item);
  });
}

async function showArchive(date) {
  viewingDate = date;
  const digest = await api(`/api/digest/${date}`);
  renderStories(digest);
  const history = await api("/api/history");
  renderHistory(history.days || []);
}

function renderStatus(status) {
  $("speechStatus").textContent = status.speech_backend === "qwen"
    ? (status.speech_installed ? (language === "zh" ? "Qwen-TTS · 本机模型" : "Qwen-TTS · local model") : (language === "zh" ? "Qwen-TTS 缺少本机模型，朗读暂不可用" : "Local Qwen-TTS model missing; speech unavailable"))
    : (status.speech_installed ? "Kokoro" : (language === "zh" ? "本机语音尚未配置" : "Local speech is not configured"));
  const connected = status.device && status.device.connected === true;
  $("deviceDot").className = connected ? "dot on" : "dot";
  $("deviceStatus").textContent = status.runtime && status.runtime.offline
    ? t("deviceDev")
    : connected ? t("deviceOn") : t("deviceOff");
  $("refresh").disabled = status.job && status.job.running;
  $("jobStatus").textContent = status.job && status.job.running
    ? t("refreshing")
    : (status.job && status.job.last_error) || "";
  if (!viewingDate) renderStories(status.digest);
  renderEink(status.view);
}

async function refresh() {
  const [status, sourceList, history] = await Promise.all([
    api("/api/status"),
    api("/api/sources"),
    api("/api/history"),
  ]);
  fillSettings(status.settings);
  renderStatus(status);
  renderSources(sourceList.sources || []);
  renderHistory(history.days || []);
}

async function device(body) {
  try {
    await api("/api/device/action", body);
    const status = await api("/api/status");
    renderStatus(status);
  } catch (error) {
    toast(error.message);
  }
}

document.querySelectorAll("[data-key]").forEach((button) => {
  button.addEventListener("click", () => device({ action: button.dataset.key }));
});

$("llmBackend").addEventListener("change", () => { $("ollamaFields").hidden = $("llmBackend").value !== "ollama"; });

$("langToggle").addEventListener("click", async () => {
  if (!settings) return;
  language = language === "zh" ? "en" : "zh";
  applyChrome();
  try {
    const next = readSettings();
    const result = await api("/api/settings", next);
    fillSettings(result.settings);
    viewingDate = "";
    toast(result.regenerating ? t("refreshing") : t("saved"));
    await refresh();
  } catch (error) {
    toast(error.message);
  }
});

$("refresh").addEventListener("click", async () => {
  try {
    viewingDate = "";
    await api("/api/refresh", {});
    $("jobStatus").textContent = t("refreshing");
  } catch (error) {
    toast(error.message);
  }
});

$("settingsForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  $("formError").textContent = "";
  try {
    const result = await api("/api/settings", readSettings());
    fillSettings(result.settings);
    toast(result.regenerating ? t("refreshing") : t("saved"));
    await refresh();
  } catch (error) {
    $("formError").textContent = error.message;
  }
});

$("saveSources").addEventListener("click", async () => {
  try {
    const result = await api("/api/settings", readSettings());
    fillSettings(result.settings);
    toast(t("saved"));
  } catch (error) {
    toast(error.message);
  }
});

$("ttsSpeed").addEventListener("input", () => {
  $("speedOut").textContent = Number($("ttsSpeed").value).toFixed(2);
});
$("volume").addEventListener("input", () => {
  $("volumeOut").textContent = $("volume").value;
});

(async () => {
  try {
    const boot = await api("/api/boot");
    token = boot.token;
    await refresh();
    setInterval(() => {
      api("/api/status").then(renderStatus).catch(() => {});
    }, 2000);
  } catch (error) {
    $("emptyTitle").textContent = error.message;
  }
})();
