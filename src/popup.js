const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);


const LANGS = [
  "简体中文",
  "繁體中文",
  "English",
  "日本語",
  "한국어",
  "Français",
  "Deutsch",
  "Español",
  "Русский",
];


const PREFS_KEY = "popup.prefs";

const state = {
  config: null,
  text: "",
  sceneId: "general",
  sourceLang: "auto",
  targetLang: "简体中文",
  requestId: "",
  output: "",
  translating: false,
  pinned: false,
};

function loadPrefs() {
  try {
    return JSON.parse(localStorage.getItem(PREFS_KEY) || "{}");
  } catch {
    return {};
  }
}

function savePrefs() {
  try {
    localStorage.setItem(
      PREFS_KEY,
      JSON.stringify({
        sceneId: state.sceneId,
        sourceLang: state.sourceLang,
        targetLang: state.targetLang,
      })
    );
  } catch {
    
  }
}

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
}



function escapeTagOpen(text) {
  return String(text).replace(/</g, "&lt;");
}

function renderMarkdown(text) {
  if (!text) return "";
  const safe = escapeTagOpen(text);
  if (window.marked && typeof window.marked.parse === "function") {
    try {
      return window.marked.parse(safe, { gfm: true, breaks: true });
    } catch {
      
    }
  }
  return safe.replace(/\n/g, "<br>");
}

let renderTimer = null;

function renderResult() {
  const el = $("result");
  if (!el) return;
  el.innerHTML = state.output ? renderMarkdown(state.output) : "";
  el.scrollTop = el.scrollHeight;
}


function scheduleRender() {
  if (renderTimer) return;
  renderTimer = setTimeout(() => {
    renderTimer = null;
    renderResult();
  }, 120);
}

function renderSceneOptions() {
  const select = $("popup-scene");
  if (!select) return;

    if (!state.config.scenes.some((s) => s.id === state.sceneId)) {
    state.sceneId = state.config.activeSceneId || state.config.scenes[0]?.id || "general";
  }

  select.innerHTML = "";
  state.config.scenes.forEach((scene) => {
    const opt = document.createElement("option");
    opt.value = scene.id;
    opt.textContent = scene.name;
    select.appendChild(opt);
  });
  select.value = state.sceneId;

  const active = state.config.scenes.find((s) => s.id === state.sceneId);
  $("scene-label").textContent = active ? active.name : "翻译";
}

function renderLangOptions() {
  const select = $("popup-lang");
  if (!select) return;

  select.innerHTML = "";
  LANGS.forEach((name) => {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    select.appendChild(opt);
  });

    if (state.targetLang && !LANGS.includes(state.targetLang)) {
    const opt = document.createElement("option");
    opt.value = state.targetLang;
    opt.textContent = state.targetLang;
    select.appendChild(opt);
  }

  select.value = state.targetLang;
}


function updateLangPair() {
  const el = $("lang-pair");
  if (!el) return;
  const from = state.sourceLang === "auto" ? "自动" : state.sourceLang;
  el.textContent = `${from} → ${state.targetLang}`;
}


function renderSourceLangOptions() {
  const select = $("popup-source-lang");
  if (!select) return;

  select.innerHTML = "";

  const auto = document.createElement("option");
  auto.value = "auto";
  auto.textContent = "自动检测";
  select.appendChild(auto);

  LANGS.forEach((name) => {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    select.appendChild(opt);
  });

  if (state.sourceLang && state.sourceLang !== "auto" && !LANGS.includes(state.sourceLang)) {
    const opt = document.createElement("option");
    opt.value = state.sourceLang;
    opt.textContent = state.sourceLang;
    select.appendChild(opt);
  }

  select.value = state.sourceLang || "auto";
}

function setPlaceholder(message, isError = false) {
  let el = $("placeholder");
  if (!el) {
    el = document.createElement("div");
    el.className = "placeholder";
    el.id = "placeholder";
    $("result").appendChild(el);
  }
  el.textContent = message;
  el.classList.toggle("error", isError);
  el.style.display = message ? "block" : "none";
}

function setBusy(busy) {
  state.translating = busy;
  $("btn-retry").disabled = busy;
}

function hasApiKey() {
  const key = state.config && state.config.api && state.config.api.apiKey;
  return typeof key === "string" && key.trim().length > 0;
}

function showApiKeyModal() {
  $("modal-apikey").hidden = false;
}

function hideApiKeyModal() {
  $("modal-apikey").hidden = true;
}

async function startTranslate(text, sceneId) {
  state.text = text;
  state.sceneId = sceneId || state.sceneId;
  state.requestId = crypto.randomUUID();
  state.output = "";

  $("source-text").textContent = text;
  $("result").innerHTML = "";
  const placeholder = document.createElement("div");
  placeholder.className = "placeholder";
  placeholder.id = "placeholder";
  placeholder.textContent = text ? "正在翻译…" : "未检测到选中的文本，请先选择内容后重试";
  $("result").appendChild(placeholder);

  updateLangPair();
  renderSceneOptions();
  renderLangOptions();
  renderSourceLangOptions();

  if (!text) {
    setBusy(false);
    return;
  }

    if (!hasApiKey()) {
    setBusy(false);
    $("result").innerHTML = "";
    setPlaceholder("尚未配置 API Key", true);
    showApiKeyModal();
    return;
  }

  setBusy(true);

  try {
    await invoke("translate", {
      req: {
        text,
        sceneId: state.sceneId,
                targetLang: state.targetLang,
        sourceLang: state.sourceLang,
        targetWindow: "popup",
        requestId: state.requestId,
      },
    });
  } catch (err) {
    setBusy(false);
    setPlaceholder(String(err), true);
  }
}

function bindEvents() {
    $("modal-cancel").addEventListener("click", hideApiKeyModal);
  $("modal-goto-settings").addEventListener("click", async () => {
    hideApiKeyModal();
        await invoke("open_settings");
  });

  $("btn-pin").addEventListener("click", async () => {
    state.pinned = !state.pinned;
    await invoke("popup_set_pinned", { pinned: state.pinned });
    $("btn-pin").classList.toggle("active", state.pinned);
    $("btn-pin").dataset.tooltip = state.pinned ? "已钉住：不会自动隐藏" : "钉住：不自动隐藏";
  });

  $("btn-copy").addEventListener("click", async () => {
    if (!state.output) return;
    await navigator.clipboard.writeText(state.output);
    const btn = $("btn-copy");
    const old = btn.textContent;
    btn.textContent = "已复制";
    setTimeout(() => (btn.textContent = old), 1200);
  });

  $("btn-retry").addEventListener("click", () => {
    if (!state.text) return;
    startTranslate(state.text, state.sceneId);
  });

  $("popup-scene").addEventListener("change", (e) => {
    state.sceneId = e.target.value;
    savePrefs();
    renderSceneOptions();
    if (state.text) startTranslate(state.text, state.sceneId);
  });

  $("popup-lang").addEventListener("change", (e) => {
    state.targetLang = e.target.value;
    savePrefs();
    updateLangPair();
    if (state.text) startTranslate(state.text, state.sceneId);
  });

  $("popup-source-lang").addEventListener("change", (e) => {
    state.sourceLang = e.target.value;
    savePrefs();
    updateLangPair();
    if (state.text) startTranslate(state.text, state.sceneId);
  });

    listen("popup-text", (event) => {
    startTranslate(event.payload || "", state.sceneId);
  });

  listen("translate-chunk", (event) => {
    if (event.payload.requestId !== state.requestId) return;
    state.output += event.payload.delta;
    scheduleRender();
  });

  listen("translate-done", (event) => {
    if (event.payload.requestId !== state.requestId) return;
    state.output = event.payload.fullText || state.output;
    renderResult();
    if (!state.output) setPlaceholder("没有拿到译文", true);
    setBusy(false);
  });

  listen("translate-error", (event) => {
    if (event.payload.requestId !== state.requestId) return;
    setBusy(false);
    $("result").innerHTML = "";
    setPlaceholder(event.payload.message, true);
  });

  listen("config-changed", (event) => {
    const cfg = event.payload;
    if (!cfg) return;
    state.config = cfg;
    applyTheme(cfg.theme);
        renderSceneOptions();
  });

    listen("need-api-key", () => {
    setBusy(false);
    showApiKeyModal();
  });
}

async function init() {
  try {
    state.config = await invoke("get_config");
  } catch (err) {
    setPlaceholder("读取配置失败：" + err, true);
    return;
  }

  applyTheme(state.config.theme);

    const prefs = loadPrefs();
  state.sceneId = prefs.sceneId || state.config.activeSceneId;
  state.sourceLang = prefs.sourceLang || state.config.languages.source || "auto";
  state.targetLang = prefs.targetLang || state.config.languages.target;

  updateLangPair();
  renderSceneOptions();
  renderLangOptions();
  renderSourceLangOptions();

  bindEvents();

    await invoke("popup_ready");
}

window.addEventListener("DOMContentLoaded", init);
