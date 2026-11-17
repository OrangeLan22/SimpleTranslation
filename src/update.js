const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

const state = {
  info: null,
  running: false,
  finished: false,
};

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
}

function formatSize(bytes) {
  if (!bytes || bytes <= 0) return "";
  const mb = bytes / 1024 / 1024;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

function setStatus(text) {
  $("status-text").textContent = text;
}

function showError(message) {
  const box = $("error-box");
  box.textContent = message;
  box.hidden = false;
}

function hideError() {
  $("error-box").hidden = true;
}

function setProgress(percent) {
  const safe = Math.max(0, Math.min(100, Math.round(percent)));
  $("progress-fill").style.width = `${safe}%`;
  $("progress-percent").textContent = `${safe}%`;
}


function renderInfo(info) {
  state.info = info;
  $("ver-new").textContent = info.version;
  $("ver-old").textContent = info.currentVersion;

  const notes = (info.notes || "").trim();
  const notesEl = $("notes");
  if (notes) {
    notesEl.textContent = notes;
    notesEl.hidden = false;
  } else {
    notesEl.hidden = true;
  }

    if (!state.running && !state.finished) {
    startDownload();
  }
}

async function startDownload() {
  if (state.running || state.finished) return;

  state.running = true;
  hideError();
  $("progress-block").hidden = false;
  setProgress(0);
  setStatus(state.info?.mock ? "正在模拟下载…" : "正在下载更新包…");
  $("btn-start").disabled = true;
  $("btn-cancel").textContent = "取消";

  try {
    await invoke("start_update");
  } catch (err) {
        state.running = false;
    $("btn-start").disabled = false;
    setStatus("更新失败");
    showError(String(err));
    await invoke("reset_mock_update").catch(() => {});
  }
}

async function cancel() {
  if (state.finished) {
    await invoke("cancel_update");
    return;
  }
    await invoke("cancel_update");
}

function bindEvents() {
  $("btn-start").addEventListener("click", () => {
        hideError();
    startDownload();
  });

  $("btn-cancel").addEventListener("click", cancel);

  listen("update-available", (event) => {
    renderInfo(event.payload || {});
  });

  listen("update-progress", (event) => {
    const { percent } = event.payload || {};
    setProgress(percent || 0);
  });

  listen("update-done", async (event) => {
    state.running = false;
    state.finished = true;

    const mock = !!event.payload?.mock;
    setProgress(100);

    if (mock) {
      setStatus("模拟更新完成（真实更新时这里会安装并重启）");
      $("btn-cancel").textContent = "关闭";
      $("btn-start").disabled = true;
      await invoke("reset_mock_update").catch(() => {});
    } else {
            setStatus("更新已就绪，正在重启…");
      $("btn-start").disabled = true;
      $("btn-cancel").disabled = true;
    }
  });
}

async function init() {
  try {
    const cfg = await invoke("get_config");
    applyTheme(cfg?.theme);
  } catch {
    applyTheme("light");
  }

  bindEvents();

      try {
    const info = await invoke("check_update");
    if (info) renderInfo(info);
  } catch {
    
  }
}

window.addEventListener("DOMContentLoaded", init);
