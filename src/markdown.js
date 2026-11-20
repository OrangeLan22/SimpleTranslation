(function () {
  "use strict";

        const CODE_SIGNALS = [
    /^\s*\/\//m,
    /\/\*|\*\//,
    /^\s*#\s*(include|define|ifdef|ifndef|endif|elif|undef|pragma|error)\b/m,
    /^\s*(def|class|func|function|fn|struct|enum|impl|trait|interface|const|let|var|import|from|export|return|public|private|protected|static|void|namespace|package|using|use|mod)\b/m,
    /=>|::|->/,
    /[{};]\s*$/m,
    /^\s*(#!|\$ |>>> |PS[ >])/m,
    /^\s*<\/?[a-zA-Z][\w-]*[^>]*>/m,
  ];

  function looksLikeCode(text) {
    const t = String(text || "").trim();
    if (!t) return false;
    return CODE_SIGNALS.some((re) => re.test(t));
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

  window.STMarkdown = { looksLikeCode, renderMarkdown, escapeTagOpen };
})();
