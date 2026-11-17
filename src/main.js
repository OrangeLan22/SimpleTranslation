const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  config: null,
  configPath: "",
  currentSceneId: "general",
  translating: false,
  requestId: "",
  output: "",
  hotkeyBuffer: "",
};

let copyDocTimer = null;

const $ = (id) => document.getElementById(id);

function toast(message, ms = 2200) {
  const el = $("toast");
  el.textContent = message;
  el.classList.add("show");
  clearTimeout(toast._timer);
  toast._timer = setTimeout(() => el.classList.remove("show"), ms);
}

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
  document.querySelectorAll("[data-theme-set]").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.themeSet === theme);
  });
  const segmented = $("theme-segmented");
  if (segmented) {
    segmented.querySelectorAll("button").forEach((b) => {
      b.classList.toggle("active", b.dataset.value === theme);
    });
  }
}

function switchView(view) {
  document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
  $(`view-${view}`).classList.add("active");
  $("btn-settings").classList.toggle("active", view === "settings");
}

function bindNavigation() {
    $("btn-settings").addEventListener("click", () => {
    const onSettings = $("view-settings").classList.contains("active");
    switchView(onSettings ? "translate" : "settings");
  });

    $("btn-back-translate").addEventListener("click", () => switchView("translate"));
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

function goToSettingsFromModal() {
  hideApiKeyModal();
  switchView("settings");
    openSettingsPanel("model");
    setTimeout(() => {
    const input = $("api-key");
    if (input) {
      input.focus();
      input.select();
    }
  }, 60);
}

function bindApiKeyModal() {
  $("modal-cancel").addEventListener("click", hideApiKeyModal);
  $("modal-goto-settings").addEventListener("click", goToSettingsFromModal);

    $("modal-apikey").addEventListener("click", (e) => {
    if (e.target === $("modal-apikey")) hideApiKeyModal();
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("modal-apikey").hidden) hideApiKeyModal();
  });
}

let confirmResolve = null;

function showConfirm(title, desc, okText = "确定") {
  $("confirm-title").textContent = title;
  $("confirm-desc").textContent = desc;
  $("confirm-ok").textContent = okText;
  $("modal-confirm").hidden = false;
  return new Promise((resolve) => {
    confirmResolve = resolve;
  });
}

function settleConfirm(result) {
  $("modal-confirm").hidden = true;
  if (confirmResolve) {
    confirmResolve(result);
    confirmResolve = null;
  }
}

function bindConfirmModal() {
  $("confirm-ok").addEventListener("click", () => settleConfirm(true));
  $("confirm-cancel").addEventListener("click", () => settleConfirm(false));
  $("modal-confirm").addEventListener("click", (e) => {
    if (e.target === $("modal-confirm")) settleConfirm(false);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("modal-confirm").hidden) settleConfirm(false);
  });
}

let autoSaveTimer = null;
let saveStatusTimer = null;



function autoSave(delay = 320) {
  clearTimeout(autoSaveTimer);
  autoSaveTimer = setTimeout(async () => {
    collectSettings();
    try {
      await persist(state.config);
      const status = $("save-status");
      if (status) {
        status.textContent = "已自动保存";
        clearTimeout(saveStatusTimer);
        saveStatusTimer = setTimeout(() => (status.textContent = ""), 1600);
      }
    } catch (err) {
      toast(String(err), 4200);
    }
  }, delay);
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

function bindTranslateView() {
  const source = $("doc-source");
  const output = $("doc-output");
  const count = $("doc-count");
  const btn = $("btn-translate");

  source.addEventListener("input", () => {
    count.textContent = `${source.value.length} 字`;
  });

  source.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      doTranslate();
    }
  });

  btn.addEventListener("click", doTranslate);

  $("btn-copy-doc").addEventListener("click", async () => {
    if (!state.output) return toast("还没有译文");
    await navigator.clipboard.writeText(state.output);

        const btn = $("btn-copy-doc");
    btn.innerHTML = ICON_CHECK;
    btn.classList.add("copied");
    clearTimeout(copyDocTimer);
    copyDocTimer = setTimeout(() => {
      btn.innerHTML = ICON_COPY;
      btn.classList.remove("copied");
    }, 1400);

    toast("译文已复制");
  });

  $("doc-target").addEventListener("change", async (e) => {
    state.config.languages.target = e.target.value;
    await persist(state.config);
  });

  $("doc-source-lang").addEventListener("change", async (e) => {
    state.config.languages.source = e.target.value;
    await persist(state.config);
  });

  $("doc-scene").addEventListener("change", async (e) => {
    state.config.activeSceneId = e.target.value;
    state.currentSceneId = e.target.value;
    await persist(state.config);
  });

  function setBusy(busy) {
    state.translating = busy;
    btn.disabled = busy;
    btn.querySelector(".label").textContent = busy ? "翻译中…" : "翻译";
    $("doc-status").textContent = busy ? "正在生成" : "";
  }

  async function doTranslate() {
    if (state.translating) return;
    const text = source.value.trim();
    if (!text) return toast("请先输入要翻译的内容");

        if (!hasApiKey()) {
      showApiKeyModal();
      return;
    }

    state.requestId = crypto.randomUUID();
    state.output = "";
    output.innerHTML = "";
    setBusy(true);

    try {
      await invoke("translate", {
        req: {
          text,
          sceneId: state.config.activeSceneId,
          targetLang: state.config.languages.target,
          sourceLang: state.config.languages.source,
          targetWindow: "main",
          requestId: state.requestId,
        },
      });
    } catch (err) {
      setBusy(false);
      toast(String(err));
    }
  }

    let renderTimer = null;

  function flushOutput() {
    output.innerHTML = state.output ? renderMarkdown(state.output) : "";
    output.scrollTop = output.scrollHeight;
  }

  function scheduleRender() {
    if (renderTimer) return;
    renderTimer = setTimeout(() => {
      renderTimer = null;
      flushOutput();
    }, 120);
  }

  listen("translate-chunk", (event) => {
    const { requestId, delta } = event.payload;
    if (requestId !== state.requestId) return;
    state.output += delta;
    scheduleRender();
  });

  listen("translate-done", (event) => {
    if (event.payload.requestId !== state.requestId) return;
    state.output = event.payload.fullText || state.output;
    if (renderTimer) {
      clearTimeout(renderTimer);
      renderTimer = null;
    }
    flushOutput();
    setBusy(false);
  });

  listen("translate-error", (event) => {
    if (event.payload.requestId !== state.requestId) return;
    setBusy(false);
    $("doc-status").textContent = "";
    toast(event.payload.message, 5000);
  });
}

function renderThemeSegmented() {
    document.querySelectorAll("[data-theme-set]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      state.config.theme = btn.dataset.themeSet;
      applyTheme(state.config.theme);
      await persist(state.config);
    });
  });

    const seg = $("theme-segmented");
  if (seg) {
    seg.querySelectorAll("button").forEach((btn) => {
      btn.addEventListener("click", async () => {
        state.config.theme = btn.dataset.value;
        applyTheme(state.config.theme);
        await persist(state.config);
      });
    });
  }
}

const HOTKEY_KEYS = ["showWindow", "translate", "disable", "enable"];

function bindHotkeys() {
  document.querySelectorAll(".hk-input").forEach((input) => {
    const slot = input.dataset.key;

    input.addEventListener("keydown", (e) => {
      e.preventDefault();
      if (e.key === "Escape") {
        input.blur();
        return;
      }
      const mods = [];
      if (e.ctrlKey) mods.push("CommandOrControl");
      if (e.altKey) mods.push("Alt");
      if (e.shiftKey) mods.push("Shift");
      if (e.metaKey) mods.push("Super");

      const code = normalizeKey(e);
      if (!code) {
        input.value = mods.length ? mods.map(friendlyMod).join(" + ") + " + …" : "";
        return;
      }
      input.value = [...mods, code].map(friendlyKey).join(" + ");
      input.dataset.raw = [...mods, code].join("+");
      autoSave();
    });

    input.addEventListener("focus", () => {
      input.dataset.raw = state.config.hotkeys?.[slot] ?? "";
    });
  });

  document.querySelectorAll(".hk-clear").forEach((btn) => {
    btn.addEventListener("click", () => {
      const input = document.querySelector(`.hk-input[data-key="${btn.dataset.key}"]`);
      if (!input) return;
      input.value = "";
      input.dataset.raw = "";
      autoSave();
    });
  });
}

function setSegmented(id, value) {
  const seg = $(id);
  if (!seg) return;
  seg.querySelectorAll("button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.value === value);
  });
}

const TRIGGER_HINTS = {
  button: "自动出现翻译按钮",
  hotkey: "按下快捷键直接翻译当前选中的内容",
  both: "适用于某些情况下悬浮按钮无法出现的情况",
};

function updateTriggerHint(mode) {
  const el = $("trigger-hint");
  if (el) el.textContent = TRIGGER_HINTS[mode] || "";
}

function normalizeKey(e) {
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return null;
  if (/^Key[A-Z]$/.test(e.code)) return e.code;
  if (/^Digit\d$/.test(e.code)) return e.code;
  if (/^Numpad\d$/.test(e.code)) return e.code;
  if (/^F\d{1,2}$/.test(e.code)) return e.code;
  const allowed = new Set([
    "Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Insert",
    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End",
    "PageUp", "PageDown", "Minus", "Equal", "BracketLeft", "BracketRight",
    "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "Backquote",
  ]);
  return allowed.has(e.code) ? e.code : null;
}

const MOD_LABELS = { CommandOrControl: "Ctrl", Alt: "Alt", Shift: "Shift", Super: "Win" };
const friendlyMod = (m) => MOD_LABELS[m] || m;
const friendlyKey = (k) => {
  if (MOD_LABELS[k]) return MOD_LABELS[k];
  if (k.startsWith("Key")) return k.slice(3);
  if (k.startsWith("Digit")) return k.slice(5);
  return k;
};


let modelFetchTimer = null;

async function loadModels({ notify = false } = {}) {
  const select = $("api-model");
  if (!select) return;

  const apiKey = $("api-key").value.trim();
  if (!apiKey) {
    select.innerHTML = `<option value="">填写 API Key 后自动获取</option>`;
    return;
  }

  const baseUrl = $("api-base").value.trim() || "https://api.deepseek.com";
  const keep = select.value || state.config.api.model;

  select.disabled = true;
  try {
    const models = await invoke("fetch_models", { baseUrl, apiKey });

    select.innerHTML = "";
    models.forEach((id) => {
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = id;
      select.appendChild(opt);
    });

        if (models.includes(keep)) {
      select.value = keep;
    } else {
      select.value = models[0];
      if (notify) toast(`已自动选择 ${models[0]}`);
    }

    state.config.api.model = select.value;
    autoSave(100);
  } catch (err) {
    if (notify) toast(String(err), 4200);
        if (keep) {
      select.innerHTML = "";
      const opt = document.createElement("option");
      opt.value = keep;
      opt.textContent = keep;
      select.appendChild(opt);
      select.value = keep;
    }
  } finally {
    select.disabled = false;
  }
}


function updateTempVisual() {
  const input = $("api-temp");
  if (!input) return;
  const min = Number(input.min) || 0;
  const max = Number(input.max) || 1;
  const val = Number(input.value) || 0;
  const pct = max > min ? ((val - min) / (max - min)) * 100 : 0;
  input.style.setProperty("--fill", `${pct}%`);
}


async function loadAutostart() {
  const toggle = $("autostart-toggle");
  if (!toggle) return;
  try {
    toggle.checked = await invoke("get_autostart");
  } catch {
    toggle.checked = false;
  }
}

function bindAutostart() {
  const toggle = $("autostart-toggle");
  if (!toggle) return;

  toggle.addEventListener("change", async () => {
    const want = toggle.checked;
    try {
      await invoke("set_autostart", { enabled: want });
      toast(want ? "已开启开机自启" : "已关闭开机自启");
    } catch (err) {
            toggle.checked = !want;
      toast(String(err), 4200);
    }
  });
}


const AUTHOR_EMAIL = "admin@orangelan.cn";
const GITHUB_URL = "https://github.com/OrangeLan22";


let pendingUpdateInfo = null;

function showUpdateBanner(info) {
  pendingUpdateInfo = info;
  const bar = $("update-banner");
  if (!bar) return;
  $("update-banner-text").textContent =
    `发现新版本 ${info.version}（当前 ${info.currentVersion}）`;
  bar.hidden = false;
}

function hideUpdateBanner() {
  const bar = $("update-banner");
  if (bar) bar.hidden = true;
}

function bindUpdateBanner() {
    $("update-banner-go")?.addEventListener("click", async () => {
    hideUpdateBanner();
    try {
      await invoke("open_update_window");
    } catch (err) {
      toast(String(err), 4200);
    }
  });

  $("update-banner-later")?.addEventListener("click", () => hideUpdateBanner());

  listen("update-available", (event) => {
    const info = event.payload;
    if (info) showUpdateBanner(info);
  });
}

function bindAbout() {
    const wechat = $("modal-wechat");
  $("contact-wechat")?.addEventListener("click", () => {
    if (wechat) wechat.hidden = false;
  });
  $("btn-close-wechat")?.addEventListener("click", () => {
    if (wechat) wechat.hidden = true;
  });
  wechat?.addEventListener("click", (e) => {
    if (e.target === wechat) wechat.hidden = true;
  });

    $("contact-email")?.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(AUTHOR_EMAIL);
      toast(`已复制邮箱：${AUTHOR_EMAIL}`);
    } catch (err) {
      toast("复制失败：" + err, 4200);
    }
  });

    $("contact-github")?.addEventListener("click", async () => {
    try {
      await invoke("open_url", { url: GITHUB_URL });
    } catch (err) {
      toast(String(err), 4200);
    }
  });

    const seg = $("update-mode");
  seg?.querySelectorAll("button").forEach((btn) => {
    btn.addEventListener("click", () => {
      state.config.update.mode = btn.dataset.value;
      setSegmented("update-mode", btn.dataset.value);
      autoSave(120);
    });
  });

  $("update-enabled")?.addEventListener("change", () => autoSave(80));

    $("btn-mock-auto")?.addEventListener("click", async () => {
    try {
      await invoke("mock_update", { mode: "auto" });
    } catch (err) {
      toast(String(err), 4200);
    }
  });

  $("btn-mock-notify")?.addEventListener("click", async () => {
    try {
      await invoke("mock_update", { mode: "notify" });
    } catch (err) {
      toast(String(err), 4200);
    }
  });
}

function bindSettings() {
    const triggerSeg = $("trigger-segmented");
  if (triggerSeg) {
    triggerSeg.querySelectorAll("button").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.config.trigger.mode = btn.dataset.value;
        setSegmented("trigger-segmented", btn.dataset.value);
        updateTriggerHint(btn.dataset.value);
        autoSave();
      });
    });
  }

    $("btn-toggle-key").addEventListener("click", () => {
    const input = $("api-key");
    const show = input.type === "password";
    input.type = show ? "text" : "password";
    $("btn-toggle-key").textContent = show ? "隐藏" : "显示";
  });

    $("api-temp").addEventListener("input", (e) => {
    $("api-temp-val").textContent = Number(e.target.value).toFixed(1);
    updateTempVisual();
    autoSave(500);
  });

    $("api-provider").addEventListener("change", () => autoSave());
  $("api-base").addEventListener("input", () => autoSave(700));
  $("api-model").addEventListener("input", () => autoSave(700));
  $("api-key").addEventListener("input", () => autoSave(700));

    const scheduleModelFetch = () => {
    clearTimeout(modelFetchTimer);
    if ($("api-key").value.trim().length < 8) return;
    modelFetchTimer = setTimeout(() => loadModels(), 900);
  };
  $("api-key").addEventListener("input", scheduleModelFetch);
  $("api-key").addEventListener("blur", () => {
    clearTimeout(modelFetchTimer);
    if ($("api-key").value.trim()) loadModels();
  });
  $("api-model").addEventListener("change", () => autoSave(100));
  $("btn-refresh-models").addEventListener("click", () => loadModels({ notify: true }));
  $("api-stream").addEventListener("change", () => autoSave());
  $("selection-threshold").addEventListener("input", () => autoSave(500));

    $("selection-enabled").addEventListener("change", () => autoSave(80));

    $("btn-test-api").addEventListener("click", async () => {
    const status = $("test-status");
    status.textContent = "测试中…";
    try {
            collectSettings();
      await persist(state.config);
      const msg = await invoke("test_api");
      status.textContent = msg;
    } catch (err) {
      status.textContent = String(err);
    }
  });

    $("btn-open-config").addEventListener("click", async () => {
    try {
      await invoke("open_config_dir");
    } catch (err) {
      toast(String(err));
    }
  });

    $("btn-reset-settings").addEventListener("click", async () => {
    const ok = await showConfirm(
      "恢复默认设置",
      "主题、触发方式、快捷键、API 配置以及全部场景提示词都会重置为初始状态，已填写的 API Key 也会被清除。此操作无法撤销。",
      "恢复默认"
    );
    if (!ok) return;

    try {
      const fresh = await invoke("reset_config");
      state.config = fresh;
      fillForms();
      toast("已恢复默认设置");
    } catch (err) {
      toast(String(err), 5000);
    }
  });
}

function collectSettings() {
  state.config.trigger.mode =
    document.querySelector("#trigger-segmented .active")?.dataset.value || "button";

  state.config.selectionEnabled = $("selection-enabled").checked;
  state.config.update.enabled = $("update-enabled").checked;

  if (!state.config.hotkeys) state.config.hotkeys = {};
  HOTKEY_KEYS.forEach((slot) => {
    const input = document.querySelector(`.hk-input[data-key="${slot}"]`);
    const raw = input?.dataset.raw ?? state.config.hotkeys[slot] ?? "";
    state.config.hotkeys[slot] = raw.trim();
  });

  state.config.api.provider = $("api-provider").value;
  state.config.api.baseUrl = $("api-base").value.trim() || "https://api.deepseek.com";
  state.config.api.model = $("api-model").value.trim() || "deepseek-chat";
  state.config.api.apiKey = $("api-key").value.trim();
  state.config.api.temperature = Number($("api-temp").value);
  state.config.stream = $("api-stream").checked;
  const threshold = Number(String($("selection-threshold").value).trim());
  state.config.selectionThreshold = Number.isFinite(threshold)
    ? Math.max(0, Math.trunc(threshold))
    : 5000;

  state.config.languages.target = $("doc-target").value;
  state.config.languages.source = $("doc-source-lang").value;
  state.config.activeSceneId = state.currentSceneId;
}

async function persist(config) {
  const path = await invoke("save_config", { config });
  state.configPath = path;
  return path;
}

function sceneById(id) {
  return state.config.scenes.find((s) => s.id === id);
}

function renderSceneList() {
  const wrap = $("scene-items");
  wrap.innerHTML = "";
  state.config.scenes.forEach((scene) => {
    const item = document.createElement("div");
    item.className = "scene-item" + (scene.id === state.currentSceneId ? " active" : "");
    item.innerHTML = `<span>${escapeHtml(scene.name)}</span>${
      scene.builtin ? '<span class="badge">内置</span>' : ""
    }`;
    item.addEventListener("click", () => selectScene(scene.id));
    wrap.appendChild(item);
  });
}

function renderSceneSelect() {
  const select = $("doc-scene");
  select.innerHTML = "";
  state.config.scenes.forEach((scene) => {
    const opt = document.createElement("option");
    opt.value = scene.id;
    opt.textContent = scene.name;
    select.appendChild(opt);
  });
  select.value = state.config.activeSceneId;
}



let promptUndo = null;
let promptUndoTimer = null;

const PROMPT_UNDO_MS = 5000;

function syncUndoButton() {
  const btn = $("btn-undo-prompt");
  if (!btn) return;
  btn.hidden = !(promptUndo && promptUndo.sceneId === state.currentSceneId);
}


function startPromptUndo(sceneId, prompt) {
  promptUndo = { sceneId, prompt };
  if (promptUndoTimer) clearTimeout(promptUndoTimer);
  promptUndoTimer = setTimeout(() => {
    promptUndoTimer = null;
    promptUndo = null;
    syncUndoButton();
  }, PROMPT_UNDO_MS);
  syncUndoButton();
}


function clearPromptUndo() {
  if (promptUndoTimer) {
    clearTimeout(promptUndoTimer);
    promptUndoTimer = null;
  }
  promptUndo = null;
  syncUndoButton();
}

function selectScene(id) {
  state.currentSceneId = id;
  const scene = sceneById(id);
  if (!scene) return;
  $("scene-name").value = scene.name;
  $("scene-prompt").value = scene.prompt;
    $("btn-reset-prompt").hidden = !scene.builtin;
      $("btn-del-scene").disabled = false;
  renderSceneList();
  syncUndoButton();
}


function insertIntoPrompt(textarea, text) {
  if (!textarea) return;

  const start = textarea.selectionStart ?? textarea.value.length;
  const end = textarea.selectionEnd ?? start;
  const value = textarea.value;

  textarea.value = value.slice(0, start) + text + value.slice(end);

  const caret = start + text.length;
  textarea.selectionStart = caret;
  textarea.selectionEnd = caret;
  textarea.focus();

    textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

function bindSceneEditor() {
  $("scene-name").addEventListener("input", (e) => {
    const scene = sceneById(state.currentSceneId);
    if (!scene) return;
    scene.name = e.target.value;
    renderSceneList();
    renderSceneSelect();
    $("doc-scene").value = state.config.activeSceneId;
    autoSave(600);
  });

  $("scene-prompt").addEventListener("input", (e) => {
    const scene = sceneById(state.currentSceneId);
    if (scene) scene.prompt = e.target.value;
    autoSave(700);
  });

    document.querySelectorAll(".var-chip").forEach((chip) => {
    chip.addEventListener("click", (e) => {
      e.preventDefault();
      insertIntoPrompt($("scene-prompt"), chip.dataset.var || "");
    });
  });

  $("btn-add-scene").addEventListener("click", () => {
    const id = "custom-" + Date.now().toString(36);
    state.config.scenes.push({
      id,
      name: "新场景",
            prompt: "",
      builtin: false,
    });
    selectScene(id);
    renderSceneSelect();
    autoSave();
  });

  $("btn-del-scene").addEventListener("click", async () => {
    const scene = sceneById(state.currentSceneId);
    if (!scene) return;

    if (scene.builtin) {
      toast("内置场景不可删除");
      return;
    }

    if (state.config.scenes.length <= 1) {
      toast("至少需要保留一个场景");
      return;
    }

    const ok = await showConfirm(
      "删除场景",
      `确定删除「${scene.name}」？这个场景的提示词会一并删除。`,
      "删除"
    );
    if (!ok) return;

    state.config.scenes = state.config.scenes.filter((s) => s.id !== scene.id);
    if (promptUndo && promptUndo.sceneId === scene.id) clearPromptUndo();

    const nextId =
      state.config.activeSceneId === scene.id
        ? state.config.scenes[0]?.id || "general"
        : state.config.activeSceneId;
    if (state.config.activeSceneId === scene.id) {
      state.config.activeSceneId = nextId;
    }

    selectScene(nextId);
    renderSceneSelect();
    autoSave();
  });

  $("btn-reset-prompt").addEventListener("click", async () => {
    const scene = sceneById(state.currentSceneId);
    if (!scene || !scene.builtin) return;

        const previous = scene.prompt;

    const prompt = await invoke("reset_scene_prompt", { sceneId: scene.id });
    if (typeof prompt !== "string") return;

    startPromptUndo(scene.id, previous);
    scene.prompt = prompt;
    $("scene-prompt").value = prompt;
    autoSave();
    toast("已恢复默认提示词（5 秒内可撤销）");
  });

  $("btn-undo-prompt").addEventListener("click", () => {
    if (!promptUndo) return;

    const scene = sceneById(promptUndo.sceneId);
    if (scene) {
      scene.prompt = promptUndo.prompt;
      if (state.currentSceneId === promptUndo.sceneId) {
        $("scene-prompt").value = promptUndo.prompt;
      }
      autoSave();
      toast("已撤销");
    }

        clearPromptUndo();
  });
}

function fillForms() {
  const cfg = state.config;

  applyTheme(cfg.theme);

    setSegmented("trigger-segmented", cfg.trigger.mode || "button");
  updateTriggerHint(cfg.trigger.mode || "button");

    $("selection-enabled").checked = cfg.selectionEnabled !== false;

    $("update-enabled").checked = cfg.update?.enabled === true;
  setSegmented("update-mode", cfg.update?.mode || "notify");

    HOTKEY_KEYS.forEach((slot) => {
    const input = document.querySelector(`.hk-input[data-key="${slot}"]`);
    if (!input) return;
    const raw = cfg.hotkeys?.[slot] || "";
    input.value = raw ? raw.split("+").map(friendlyKey).join(" + ") : "";
    input.dataset.raw = raw;
  });

    $("api-provider").value = cfg.api.provider || "deepseek";
  $("api-base").value = cfg.api.baseUrl;
    const modelSelect = $("api-model");
  const savedModel = cfg.api.model || "deepseek-chat";
  modelSelect.innerHTML = "";
  const savedOpt = document.createElement("option");
  savedOpt.value = savedModel;
  savedOpt.textContent = savedModel;
  modelSelect.appendChild(savedOpt);
  modelSelect.value = savedModel;
  $("api-key").value = cfg.api.apiKey;
  $("api-temp").value = cfg.api.temperature;
  $("api-temp-val").textContent = Number(cfg.api.temperature).toFixed(1);
  updateTempVisual();
  $("api-stream").checked = !!cfg.stream;
  $("selection-threshold").value = cfg.selectionThreshold ?? 5000;

    const target = $("doc-target");
  if (![...target.options].some((o) => o.value === cfg.languages.target)) {
    const opt = document.createElement("option");
    opt.value = cfg.languages.target;
    opt.textContent = cfg.languages.target;
    target.appendChild(opt);
  }
  target.value = cfg.languages.target;

    const srcLang = $("doc-source-lang");
  const cfgSource = cfg.languages.source || "auto";
  if (![...srcLang.options].some((o) => o.value === cfgSource)) {
    const opt = document.createElement("option");
    opt.value = cfgSource;
    opt.textContent = cfgSource;
    srcLang.appendChild(opt);
  }
  srcLang.value = cfgSource;

  state.currentSceneId = cfg.activeSceneId || cfg.scenes[0]?.id;
  renderSceneSelect();
  renderSceneList();
  selectScene(state.currentSceneId);

  $("config-path").textContent = state.configPath || "读取中…";
}

function escapeHtml(str) {
  return String(str).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

const ICON_COPY = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
  <rect x="9" y="9" width="11" height="11" rx="2"/>
  <path d="M5 15V6a2 2 0 0 1 2-2h9" stroke-linecap="round"/>
</svg>`;

const ICON_CHECK = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
  <path d="M5 12.5l4.5 4.5L19 7" stroke-linecap="round" stroke-linejoin="round"/>
</svg>`;

const ICON_MAXIMIZE = `<svg viewBox="0 0 16 16" aria-hidden="true">
  <rect x="3.6" y="3.6" width="8.8" height="8.8" rx="1.4" fill="none" stroke="currentColor" stroke-width="1.3"/>
</svg>`;

const ICON_RESTORE = `<svg viewBox="0 0 16 16" aria-hidden="true">
  <rect x="2.6" y="5.4" width="8" height="8" rx="1.3" fill="none" stroke="currentColor" stroke-width="1.3"/>
  <path d="M5.4 5.4V4.1A1.5 1.5 0 0 1 6.9 2.6h5A1.5 1.5 0 0 1 13.4 4.1v5a1.5 1.5 0 0 1-1.5 1.5h-1.3"
        fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
</svg>`;

function bindWindowControls() {
  const maxBtn = $("win-max");

  const renderMaxState = (maximized) => {
    maxBtn.innerHTML = maximized ? ICON_RESTORE : ICON_MAXIMIZE;
    document.body.classList.toggle("is-maximized", maximized);
  };

  $("win-min").addEventListener("click", () => invoke("window_minimize"));
  $("win-close").addEventListener("click", () => invoke("close_main"));

  maxBtn.addEventListener("click", async () => {
    renderMaxState(await invoke("window_toggle_maximize"));
  });

    let resizeTimer = null;
  window.addEventListener("resize", () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(async () => {
      try {
        renderMaxState(await invoke("window_is_maximized"));
      } catch (_) {
        
      }
    }, 60);
  });

  invoke("window_is_maximized").then(renderMaxState).catch(() => {});
}


function openSettingsPanel(key) {
  document.querySelectorAll(".settings-nav-item").forEach((el) => {
    el.classList.toggle("active", el.dataset.panel === key);
  });
  document.querySelectorAll(".settings-panel").forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.panel === key);
  });
    $("settings-panels")?.scrollTo({ top: 0 });
}

function bindSettingsNav() {
  const nav = document.querySelector(".settings-nav");
  if (!nav) return;

  nav.addEventListener("click", (event) => {
    const item = event.target.closest(".settings-nav-item");
    if (!item) return;
    openSettingsPanel(item.dataset.panel);
  });
}

async function init() {
  bindNavigation();
  bindApiKeyModal();
  bindConfirmModal();
  bindWindowControls();
  bindTranslateView();
  renderThemeSegmented();
  bindHotkeys();
  bindSettings();
  bindSettingsNav();
  bindAutostart();
  bindAbout();
  bindUpdateBanner();
  bindSceneEditor();

  try {
    state.config = await invoke("get_config");
    state.configPath = await invoke("get_config_path");
  } catch (err) {
    toast("读取配置失败：" + err, 6000);
    return;
  }

  fillForms();

    if (hasApiKey()) {
    setTimeout(() => loadModels(), 400);
  }

    loadAutostart();

    try {
    const updatedTo = await invoke("take_pending_update");
    if (updatedTo) toast(`已更新到 ${updatedTo}`, 4200);
  } catch {
    
  }

    listen("config-changed", (event) => {
    const cfg = event.payload;
    if (!cfg) return;
    if (cfg.theme !== state.config.theme) {
      state.config.theme = cfg.theme;
      applyTheme(cfg.theme);
    }
    state.config.api = cfg.api;

        if (typeof cfg.selectionEnabled === "boolean") {
      state.config.selectionEnabled = cfg.selectionEnabled;
      const toggle = $("selection-enabled");
      if (toggle) toggle.checked = cfg.selectionEnabled;
    }
  });

    listen("need-api-key", () => showApiKeyModal());

    listen("switch-to-settings", () => goToSettingsFromModal());
}

window.addEventListener("DOMContentLoaded", init);
