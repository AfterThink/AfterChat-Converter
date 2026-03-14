// 使用 window.__TAURI__ 获取 API
const { open: openDialog, save } = window.__TAURI__.dialog;
const { invoke } = window.__TAURI__.tauri;
const { appWindow } = window.__TAURI__.window;
const { getVersion } = window.__TAURI__.app;

const STORAGE_KEY = "converters.output-mode";
const NON_DRAGGABLE_SELECTOR = [
  "button",
  "input",
  "label",
  "a",
  "summary",
  "details",
  "pre",
  "textarea",
  "select",
  "[data-no-window-drag]",
].join(", ");

// i18n 配置
const i18n = {
  en: {
    inplace: "In-place",
    hint: "Drop files here",
    reveal: "Reveal",
    selecting: "Select destination...",
    processing: "Processing...",
    success: "Done",
    error: "Failed",
    errorTitle: "Why it failed",
    switchToZh: "Switch to Chinese",
    switchToEn: "Switch to English",
    aboutDesc: "A simple tool to convert conversation history JSON files to Markdown.",
    supportedFormats: "Supported Formats:"
  },
  zh: {
    inplace: "原位生成",
    hint: "拖拽文件至此",
    reveal: "打开位置",
    selecting: "选择保存位置...",
    processing: "处理中...",
    success: "完成",
    error: "失败",
    errorTitle: "失败原因",
    switchToZh: "切换到中文",
    switchToEn: "切换到英文",
    aboutDesc: "一个简单的工具，用于将对话历史 JSON 文件转换为 Markdown。",
    supportedFormats: "支持的格式："
  }
};

const ICONS = {
  ready: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
  `,
  hovering: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M8 12h8"/><path d="M12 8v8"/></svg>
  `,
  selecting: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
  `,
  processing: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>
  `,
  success: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>
  `,
  error: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
  `,
};

const state = {
  phase: "ready",
  isBusy: false,
  revealPath: "",
  errorMessage: "",
  resetTimer: null,
  outputBesideSource: loadOutputMode(),
  lang: loadLang(),
};

const elements = {
  card: document.querySelector("#drop-card"),
  icon: document.querySelector("#state-icon"),
  detailsPanel: document.querySelector("#details-panel"),
  detailsText: document.querySelector("#details-text"),
  revealButton: document.querySelector("#reveal-button"),
  outputToggle: document.querySelector("#output-toggle"),
  // i18n 元素
  inplaceLabel: document.querySelector("#i18n-inplace"),
  hintLabel: document.querySelector("#i18n-hint"),
  revealLabel: document.querySelector("#i18n-reveal"),
  // 新按钮
  btnHelp: document.querySelector("#btn-help"),
  btnLang: document.querySelector("#btn-lang"),
  currentLangText: document.querySelector("#current-lang"),
  // Modal elements
  helpModal: document.querySelector("#help-modal"),
  modalClose: document.querySelector("#modal-close"),
  appVersion: document.querySelector("#app-version"),
  aboutDesc: document.querySelector("#i18n-about-desc"),
  supportedFormats: document.querySelector("#i18n-supported-formats"),
  errorTitle: document.querySelector("#i18n-error-title"),
  btnClose: document.querySelector("#btn-close"),
  dragRegions: document.querySelectorAll("[data-window-drag]"),
};

// 获取翻译
function getT() {
  return i18n[state.lang];
}

// 初始化 i18n 文本
function updateI18nUI() {
  const t = getT();
  elements.inplaceLabel.textContent = t.inplace;
  elements.revealLabel.textContent = t.reveal;
  elements.errorTitle.textContent = t.errorTitle;
  elements.currentLangText.textContent = state.lang === "zh" ? "中" : "EN";
  elements.btnLang.title = state.lang === "zh" ? t.switchToEn : t.switchToZh;
  elements.btnLang.setAttribute("aria-label", elements.btnLang.title);
  // Update modal texts
  if (elements.aboutDesc) elements.aboutDesc.textContent = t.aboutDesc;
  if (elements.supportedFormats) elements.supportedFormats.textContent = t.supportedFormats;
}

function openHelpModal() {
  elements.helpModal.classList.add("is-open");
  elements.helpModal.setAttribute("aria-hidden", "false");
}

function closeHelpModal() {
  elements.helpModal.classList.remove("is-open");
  elements.helpModal.setAttribute("aria-hidden", "true");
}

// 语言切换逻辑
elements.btnLang.addEventListener("click", () => {
  state.lang = state.lang === "zh" ? "en" : "zh";
  localStorage.setItem("converters.lang", state.lang);
  updateI18nUI();
  render();
});

// 关闭逻辑
elements.btnClose.addEventListener("click", async () => {
  try {
    await appWindow.close();
  } catch (error) {
    console.error("Failed to close window", error);
  }
});

// 帮助逻辑
elements.btnHelp.addEventListener("click", () => {
  openHelpModal();
});

elements.modalClose.addEventListener("click", () => {
  closeHelpModal();
});

elements.helpModal.addEventListener("click", (e) => {
  if (e.target === elements.helpModal) {
    closeHelpModal();
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && elements.helpModal.classList.contains("is-open")) {
    closeHelpModal();
  }
});

// 获取版本号
getVersion().then(version => {
  if (elements.appVersion) {
    elements.appVersion.textContent = version;
  }
});

function bindWindowDragging() {
  for (const dragRegion of elements.dragRegions) {
    dragRegion.addEventListener("mousedown", async (event) => {
      if (event.button !== 0) return;
      const target = event.target instanceof Element ? event.target : dragRegion;
      if (target.closest(NON_DRAGGABLE_SELECTOR)) return;

      event.preventDefault();
      try {
        await appWindow.startDragging();
      } catch (error) {
        console.error("Failed to start window drag", error);
      }
    });
  }
}

function normalizeErrorMessage(error) {
  if (!error) return "Unknown error";
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;

  try {
    return JSON.stringify(error, null, 2);
  } catch (_) {
    return String(error);
  }
}

function showError(message) {
  state.isBusy = false;
  state.phase = "error";
  state.revealPath = "";
  state.errorMessage = message;
  render();
}

function loadLang() {
  const saved = localStorage.getItem("converters.lang");
  if (saved) return saved;
  return navigator.language.startsWith("zh") ? "zh" : "en";
}

// 初始化开关状态
elements.outputToggle.checked = state.outputBesideSource;
elements.outputToggle.addEventListener("change", (e) => {
  state.outputBesideSource = e.target.checked;
  localStorage.setItem(STORAGE_KEY, String(state.outputBesideSource));
});

elements.revealButton.addEventListener("click", async () => {
  if (state.revealPath) {
    try {
      await invoke("reveal_in_file_manager", { path: state.revealPath });
    } catch (e) {}
  }
});

appWindow.onFileDropEvent(async (event) => {
  const payload = event.payload;
  if (state.isBusy && payload.type === "drop") return;

  if (payload.type === "hover") {
    clearResetTimer();
    if (!state.isBusy) {
      state.phase = "hovering";
      render();
    }
    return;
  }

  if (payload.type === "cancel") {
    if (!state.isBusy && state.phase !== "success" && state.phase !== "error") {
      resetToReady();
    }
    return;
  }

  if (payload.type === "drop") {
    await handleDrop(payload.paths);
  }
});

async function handleDrop(paths) {
  if (!paths?.length) return;
  clearResetTimer();

  const targetPath = paths[0];
  state.isBusy = true;
  state.revealPath = "";
  state.errorMessage = "";
  state.phase = "processing";
  render();

  try {
    const input = await invoke("inspect_input", { path: targetPath });
    const converter = input.converterKind || selectConverter(input);
    
    const outputPath = await resolveOutputPath(input, converter);
    if (outputPath === null) {
      resetToReady();
      return;
    }

    const result = await invoke("run_conversion", {
      request: {
        converter,
        inputPath: input.path,
        outputPath: outputPath || undefined,
      },
    });

    state.isBusy = false;

    if (result.exitCode === 0) {
      state.revealPath = result.outputPath;
      state.phase = "success";
      render();
      scheduleReset();
    } else {
      showError([result.stderr, result.stdout].filter(Boolean).join("\n") || "Conversion failed.");
    }
  } catch (error) {
    showError(normalizeErrorMessage(error));
  }
}

async function resolveOutputPath(input, converter) {
  if (state.outputBesideSource) return undefined;

  state.phase = "selecting";
  render();

  const t = getT();
  const isDirOutput = input.isDir || converter === "cherry";

  if (isDirOutput) {
    const picked = await openDialog({
      directory: true,
      defaultPath: input.path,
      title: t.selecting,
    });
    return Array.isArray(picked) ? picked[0] : (picked || null);
  }

  const picked = await save({
    defaultPath: buildSuggestedPath(input),
    title: t.selecting,
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  return picked || null;
}

function buildSuggestedPath(input) {
  const name = input.name;
  const dotIndex = name.lastIndexOf(".");
  return dotIndex > 0 ? `${name.slice(0, dotIndex)}.md` : `${name}.md`;
}

function selectConverter(input) {
  const name = input.name.toLowerCase();
  if (name.includes("cherry")) return "cherry";
  if (name.includes("qwen")) return "qwen";
  return "ai-studio";
}

function loadOutputMode() {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === null ? true : saved === "true";
}

function resetToReady() {
  state.isBusy = false;
  state.phase = "ready";
  state.revealPath = "";
  state.errorMessage = "";
  render();
}

function scheduleReset() {
  clearResetTimer();
  state.resetTimer = setTimeout(resetToReady, 4000);
}

function clearResetTimer() {
  if (state.resetTimer) clearTimeout(state.resetTimer);
}

function render() {
  const t = getT();
  elements.card.dataset.phase = state.phase;
  elements.icon.innerHTML = ICONS[state.phase];
  
  // 更新状态文字
  if (state.phase === "ready") elements.hintLabel.textContent = t.hint;
  else if (state.phase === "selecting") elements.hintLabel.textContent = t.selecting;
  else if (state.phase === "processing") elements.hintLabel.textContent = t.processing;
  else if (state.phase === "success") elements.hintLabel.textContent = t.success;
  else if (state.phase === "error") elements.hintLabel.textContent = t.error;

  elements.detailsText.textContent = state.errorMessage;
  elements.detailsPanel.classList.toggle("hidden", state.phase !== "error" || !state.errorMessage);
  elements.revealButton.classList.toggle("hidden", state.phase !== "success" || !state.revealPath);
}

updateI18nUI();
bindWindowDragging();
render();
