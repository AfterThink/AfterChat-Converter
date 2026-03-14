import { open as openDialog, save } from "@tauri-apps/api/dialog";
import { invoke } from "@tauri-apps/api/tauri";
import { appWindow } from "@tauri-apps/api/window";

const STORAGE_KEY = "converters.inline-output";
const AUTO_RESET_MS = 4200;

const PHASE_COPY = {
  ready: {
    icon: "⇪",
    eyebrow: "等待文件",
    title: "将文件或文件夹拖拽至此",
    description: "松手后立即开始转换，不需要额外按钮。",
  },
  hovering: {
    icon: "↓",
    eyebrow: "准备接收",
    title: "松手即可开始",
    description: "我会根据名称自动选择合适的转换器。",
  },
  selecting: {
    icon: "⋯",
    eyebrow: "等待路径",
    title: "请选择保存位置",
    description: "一旦确认路径，就会立刻开始处理。",
  },
  processing: {
    icon: "○",
    eyebrow: "处理中",
    title: "正在转换…",
    description: "请稍等，我正在调用对应的后端转换器。",
  },
  success: {
    icon: "✓",
    eyebrow: "转换完成",
    title: "已成功生成输出",
    description: "你可以直接打开输出位置继续查看结果。",
  },
  error: {
    icon: "✕",
    eyebrow: "转换失败",
    title: "这次没有完成",
    description: "可展开查看错误详情，然后重试。",
  },
};

const converterLabels = {
  "ai-studio": "Google AI Studio",
  cherry: "Cherry Studio",
  qwen: "Qwen",
};

const state = {
  phase: "ready",
  outputBesideSource: loadOutputMode(),
  statusLine:
    "拖入包含 cherry / qwen 的文件会自动路由到对应转换器。",
  detailText: "",
  revealPath: "",
  currentItem: "",
  currentConverter: "",
  resetTimer: null,
  isBusy: false,
};

const elements = {
  card: document.querySelector("#drop-card"),
  icon: document.querySelector("#state-icon"),
  eyebrow: document.querySelector("#eyebrow"),
  title: document.querySelector("#title"),
  description: document.querySelector("#description"),
  outputToggle: document.querySelector("#output-toggle"),
  statusLine: document.querySelector("#status-line"),
  detailsPanel: document.querySelector("#details-panel"),
  detailsText: document.querySelector("#details-text"),
  revealButton: document.querySelector("#reveal-button"),
};

elements.outputToggle.checked = state.outputBesideSource;
elements.outputToggle.addEventListener("change", (event) => {
  state.outputBesideSource = event.currentTarget.checked;
  localStorage.setItem(STORAGE_KEY, String(state.outputBesideSource));

  if (!state.isBusy && state.phase === "ready") {
    state.statusLine = state.outputBesideSource
      ? "将直接在原位置旁输出结果。"
      : "拖入后会立即弹出原生保存对话框。";
    render();
  }
});

elements.revealButton.addEventListener("click", async () => {
  if (!state.revealPath) {
    return;
  }

  try {
    await invoke("reveal_in_file_manager", { path: state.revealPath });
  } catch (error) {
    state.statusLine = extractErrorMessage(error, "无法打开输出位置。");
    render();
  }
});

appWindow.onFileDropEvent(async (event) => {
  const payload = event.payload;

  if (state.isBusy && payload.type === "drop") {
    state.statusLine = "当前仍在处理中，请稍等这一轮结束。";
    render();
    return;
  }

  if (payload.type === "hover") {
    clearResetTimer();
    if (!state.isBusy) {
      state.phase = "hovering";
      state.statusLine = "松手后会立刻开始处理。";
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

render();

async function handleDrop(paths) {
  clearResetTimer();

  if (!paths?.length) {
    return;
  }

  const targetPath = paths[0];
  const ignoredCount = Math.max(paths.length - 1, 0);

  try {
    const input = await invoke("inspect_input", { path: targetPath });
    const converter = selectConverter(input);
    const converterLabel = converterLabels[converter];
    const outputPath = await resolveOutputPath(input, converter);

    if (outputPath === null) {
      resetToReady("已取消选择保存位置。");
      return;
    }

    state.isBusy = true;
    state.phase = "processing";
    state.currentItem = input.name;
    state.currentConverter = converterLabel;
    state.detailText = "";
    state.revealPath = "";
    state.statusLine = ignoredCount
      ? `使用 ${converterLabel} 处理 ${input.name}，其余 ${ignoredCount} 个项目已忽略。`
      : `使用 ${converterLabel} 处理 ${input.name}。`;
    render();

    const result = await invoke("run_conversion", {
      request: {
        converter,
        inputPath: input.path,
        outputPath,
      },
    });

    const mergedDetail = [result.stderr, result.stdout]
      .filter(Boolean)
      .join("\n")
      .trim();

    state.isBusy = false;
    state.revealPath = result.outputPath;

    if (result.exitCode === 0) {
      state.phase = "success";
      state.detailText = (result.stderr || "").trim();
      state.statusLine = `${converterLabel} 已完成：${input.name}`;
      render();
      scheduleReset();
      return;
    }

    state.phase = "error";
    state.detailText = mergedDetail;
    state.statusLine =
      mergedDetail || `${converterLabel} 返回了非零退出码 (${result.exitCode})。`;
    render();
    scheduleReset();
  } catch (error) {
    state.isBusy = false;
    state.phase = "error";
    state.detailText = extractErrorMessage(error, "发生了未预期的错误。");
    state.statusLine = state.detailText;
    state.revealPath = "";
    render();
    scheduleReset();
  }
}

async function resolveOutputPath(input, converter) {
  if (state.outputBesideSource) {
    return undefined;
  }

  state.phase = "selecting";
  state.currentItem = input.name;
  state.currentConverter = converterLabels[converter];
  state.statusLine = `请为 ${input.name} 选择保存位置。`;
  render();

  const shouldPickDirectory = input.isDir || converter === "cherry";

  if (shouldPickDirectory) {
    const pickedDirectory = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: input.path,
      title: "请选择保存位置",
    });

    return normalizeDialogResult(pickedDirectory);
  }

  const suggestedName = buildSuggestedFileName(input.name);
  const pickedFile = await save({
    defaultPath: suggestedName,
    title: "请选择保存位置",
    filters: [
      {
        name: "Markdown",
        extensions: ["md"],
      },
    ],
  });

  return normalizeDialogResult(pickedFile);
}

function selectConverter(input) {
  if (!input.isDir) {
    const normalizedName = input.name.toLowerCase();
    if (normalizedName.includes("cherry")) {
      return "cherry";
    }
    if (normalizedName.includes("qwen")) {
      return "qwen";
    }
  }

  return "ai-studio";
}

function buildSuggestedFileName(name) {
  const trimmed = name.trim();
  if (!trimmed) {
    return "output.md";
  }

  const suffixIndex = trimmed.lastIndexOf(".");
  if (suffixIndex <= 0) {
    return `${trimmed}.md`;
  }

  return `${trimmed.slice(0, suffixIndex)}.md`;
}

function normalizeDialogResult(value) {
  if (!value) {
    return null;
  }

  if (Array.isArray(value)) {
    return value[0] ?? null;
  }

  return value;
}

function loadOutputMode() {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === null) {
    return true;
  }

  return saved === "true";
}

function resetToReady(message) {
  state.isBusy = false;
  state.phase = "ready";
  state.detailText = "";
  state.revealPath = "";
  state.currentItem = "";
  state.currentConverter = "";
  state.statusLine =
    message || "拖入包含 cherry / qwen 的文件会自动路由到对应转换器。";
  render();
}

function scheduleReset() {
  clearResetTimer();
  state.resetTimer = window.setTimeout(() => {
    resetToReady();
  }, AUTO_RESET_MS);
}

function clearResetTimer() {
  if (state.resetTimer) {
    window.clearTimeout(state.resetTimer);
    state.resetTimer = null;
  }
}

function render() {
  const copy = PHASE_COPY[state.phase];

  elements.card.dataset.phase = state.phase;
  elements.icon.textContent = copy.icon;
  elements.icon.classList.toggle("is-spinning", state.phase === "processing");
  elements.eyebrow.textContent = state.currentConverter
    ? `${copy.eyebrow} · ${state.currentConverter}`
    : copy.eyebrow;
  elements.title.textContent = copy.title;
  elements.description.textContent = state.currentItem
    ? `${copy.description} 当前项目：${state.currentItem}`
    : copy.description;
  elements.statusLine.textContent = state.statusLine;
  elements.detailsText.textContent = state.detailText;
  elements.detailsPanel.classList.toggle("hidden", !state.detailText);
  elements.revealButton.classList.toggle(
    "hidden",
    !(state.phase === "success" && state.revealPath),
  );
}

function extractErrorMessage(error, fallback) {
  if (!error) {
    return fallback;
  }

  if (typeof error === "string") {
    return error;
  }

  if (typeof error === "object" && "message" in error && error.message) {
    return String(error.message);
  }

  return fallback;
}
