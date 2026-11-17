
(function () {
  "use strict";

  const SHOW_DELAY = 380;
  const GAP = 8;
  const EDGE = 8;

  let tip = null;
  let showTimer = null;

  function ensureTip() {
    if (!tip) {
      tip = document.createElement("div");
      tip.className = "app-tooltip";
      document.body.appendChild(tip);
    }
    return tip;
  }

  function hide() {
    if (showTimer) {
      clearTimeout(showTimer);
      showTimer = null;
    }
    if (tip) {
      tip.classList.remove("visible");
    }
  }

  function show(target, text) {
    const el = ensureTip();
    el.textContent = text;

        el.classList.remove("visible");
    el.style.left = "-9999px";
    el.style.top = "-9999px";

    const tw = el.offsetWidth;
    const th = el.offsetHeight;
    const rect = target.getBoundingClientRect();

        let x = rect.left + rect.width / 2 - tw / 2;
    let y = rect.bottom + GAP;

    if (y + th > window.innerHeight - EDGE) {
      y = rect.top - th - GAP;
    }
    if (y < EDGE) y = EDGE;
    if (x < EDGE) x = EDGE;
    if (x + tw > window.innerWidth - EDGE) {
      x = window.innerWidth - tw - EDGE;
    }

    el.style.left = Math.round(x) + "px";
    el.style.top = Math.round(y) + "px";
    el.classList.add("visible");
  }

  function targetOf(node) {
    if (!node || typeof node.closest !== "function") return null;
    return node.closest("[data-tooltip]");
  }

  document.addEventListener("mouseover", (e) => {
    const target = targetOf(e.target);
    if (!target) return;
        if (tip && tip.classList.contains("visible") && tip.dataset.owner === (target.dataset.tooltip || "")) {
      return;
    }
    clearTimeout(showTimer);
    const text = target.getAttribute("data-tooltip");
    if (!text) return;
    showTimer = setTimeout(() => {
      show(target, text);
      if (tip) tip.dataset.owner = text;
    }, SHOW_DELAY);
  });

  document.addEventListener("mouseout", (e) => {
    const target = targetOf(e.target);
    if (!target) return;
        const to = e.relatedTarget;
    if (to && target.contains(to)) return;
    hide();
  });

    document.addEventListener("mousedown", hide, true);
  document.addEventListener("wheel", hide, true);
  window.addEventListener("blur", hide);
})();
