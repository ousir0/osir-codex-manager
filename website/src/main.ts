import "./styles/redesign.css";

type Lang = "zh" | "en";
let lang: Lang = (localStorage.getItem("cam-site-lang") as Lang) || "zh";

const applyLang = (next: Lang) => {
  lang = next;
  localStorage.setItem("cam-site-lang", next);
  document.documentElement.lang = next === "zh" ? "zh-CN" : "en";
  document.title = next === "zh"
    ? "Codex Manager | 安装、配置和更新 Codex"
    : "Codex Manager | Install, configure, and update Codex";
  const description = document.querySelector<HTMLMetaElement>('meta[name="description"]');
  if (description) {
    description.content = next === "zh"
      ? "Codex Manager 是官方 Codex 桌面应用的本地管理器，支持 macOS、Windows、API 配置、OpenCodex 多模型、主题和自动更新。"
      : "Codex Manager is a local manager for the official Codex desktop app, with macOS and Windows installers, API configuration, OpenCodex multi-model routing, themes, and self-update.";
  }
  document.querySelectorAll<HTMLElement>("[data-zh][data-en]").forEach((el) => {
    el.innerHTML = el.dataset[next] || "";
  });
  const button = document.querySelector<HTMLButtonElement>("#lang-switch");
  if (button) button.textContent = next === "zh" ? "EN" : "中文";
};

applyLang(lang);
document.querySelector("#lang-switch")?.addEventListener("click", () => applyLang(lang === "zh" ? "en" : "zh"));

type CommunityTheme = {
  id: string;
  name: string;
  author: string;
  version: string;
  license: string;
  rightsStatus: "redistributable" | "source-direct" | "review-required";
  installable: boolean;
  appearance: string | null;
  bytes: number;
  sha256: string;
  preview: string;
  previews?: string[];
  sourceUrl: string;
  colors: Record<string, string>;
  art?: { focusX?: number; focusY?: number };
  previewStyle?: {
    opacity?: number;
    blur?: number;
    radius?: number;
    borderAlpha?: number;
    shadow?: "none" | "soft" | "standard";
    parts?: string[];
    hover?: boolean;
    focusVisible?: boolean;
  };
};

const COMMUNITY_CATALOG = "https://osirvedio.cn-nb1.rains3.com/codex-skins/dreamskin/v1/index.json";
const COMMUNITY_BASE = "https://osirvedio.cn-nb1.rains3.com/codex-skins/dreamskin/v1/";
const COMMUNITY_PAGE_SIZE = 12;
let communityThemes: CommunityTheme[] = [];
let communityQuery = "";
let communityPage = 0;

const communityGrid = document.querySelector<HTMLElement>("#community-theme-grid");
const communityPages = document.querySelector<HTMLElement>("#community-theme-pages");
const communityCount = document.querySelector<HTMLElement>("#community-theme-count");
const communitySearch = document.querySelector<HTMLInputElement>("#community-theme-search");
const communityDialog = document.querySelector<HTMLDialogElement>("#community-theme-dialog");
const communityDetail = document.querySelector<HTMLElement>("#community-theme-detail");

const rightsLabel = (status: CommunityTheme["rightsStatus"]) =>
  status === "redistributable" ? "可公开镜像" : status === "source-direct" ? "来源直链" : "来源安装";
const sizeLabel = (bytes: number) => (bytes / 1024 / 1024).toFixed(2) + " MB";

function appendText(parent: HTMLElement, tag: string, text: string, className?: string) {
  const element = document.createElement(tag);
  element.textContent = text;
  if (className) element.className = className;
  parent.appendChild(element);
  return element;
}

type CodexPreviewMode = "welcome" | "conversation";

function themeColor(theme: CommunityTheme, key: string, fallback: string) {
  return theme.colors?.[key] || fallback;
}

function buildCodexPreview(theme: CommunityTheme, mode: CodexPreviewMode, large = false) {
  const preview = document.createElement("div");
  preview.className = "codex-skin-preview" + (large ? " is-large" : " is-card");
  preview.style.setProperty("--skin-bg", themeColor(theme, "background", "#111318"));
  preview.style.setProperty("--skin-panel", themeColor(theme, "panel", "#202329"));
  preview.style.setProperty("--skin-panel-alt", themeColor(theme, "panelAlt", themeColor(theme, "panel", "#292d34")));
  preview.style.setProperty("--skin-accent", themeColor(theme, "accent", "#f0a84a"));
  preview.style.setProperty("--skin-highlight", themeColor(theme, "highlight", themeColor(theme, "accentAlt", "#f0a84a")));
  preview.style.setProperty("--skin-text", themeColor(theme, "text", "#f4f5f7"));
  preview.style.setProperty("--skin-muted", themeColor(theme, "muted", "#9da3ad"));
  preview.style.setProperty("--skin-line", themeColor(theme, "line", "rgba(255,255,255,.14)"));
  preview.style.setProperty("--skin-focus-x", Math.max(0, Math.min(1, Number(theme.art?.focusX ?? 0.5))) * 100 + "%");
  preview.style.setProperty("--skin-focus-y", Math.max(0, Math.min(1, Number(theme.art?.focusY ?? 0.5))) * 100 + "%");
  preview.style.setProperty("--skin-opacity", String(theme.previewStyle?.opacity ?? 1));
  preview.style.setProperty("--skin-blur", (theme.previewStyle?.blur ?? 0) + "px");
  preview.style.setProperty("--skin-radius", (theme.previewStyle?.radius ?? 12) + "px");
  preview.style.setProperty("--skin-border-alpha", String(theme.previewStyle?.borderAlpha ?? 0.14));
  preview.style.setProperty("--skin-shadow", theme.previewStyle?.shadow === "none" ? "none" : theme.previewStyle?.shadow === "standard" ? "0 12px 28px rgba(0,0,0,.34)" : "0 8px 22px rgba(0,0,0,.22)");

  const backdrop = document.createElement("img");
  backdrop.className = "codex-skin-backdrop";
  backdrop.src = COMMUNITY_BASE + theme.preview;
  backdrop.alt = "";
  backdrop.loading = large ? "eager" : "lazy";
  const focusX = Math.max(0, Math.min(1, Number(theme.art?.focusX ?? 0.5)));
  const focusY = Math.max(0, Math.min(1, Number(theme.art?.focusY ?? 0.5)));
  backdrop.style.objectPosition = focusX * 100 + "% " + focusY * 100 + "%";
  preview.appendChild(backdrop);

  const top = document.createElement("div");
  top.className = "codex-skin-top";
  const traffic = document.createElement("span");
  traffic.className = "codex-skin-traffic";
  traffic.append(document.createElement("i"), document.createElement("i"), document.createElement("i"));
  top.appendChild(traffic);
  appendText(top, "span", "▣", "codex-skin-window-icon");
  appendText(top, "span", "‹   ›", "codex-skin-history");
  preview.appendChild(top);

  const sidebar = document.createElement("aside");
  sidebar.className = "codex-skin-sidebar";
  appendText(sidebar, "strong", "Codex");
  ["✎  新对话", "‹›  拉取请求", "◷  定时任务", "✣  插件"].forEach((label) => appendText(sidebar, "span", label));
  appendText(sidebar, "small", "项目");
  appendText(sidebar, "span", "▢  DreamSkin", "active");
  ["给博客挑一套配色", "三道晚餐菜谱", "十月训练计划"].forEach((label) => appendText(sidebar, "em", label));
  appendText(sidebar, "span", "⚙  设置", "codex-skin-settings");
  preview.appendChild(sidebar);

  const main = document.createElement("main");
  main.className = "codex-skin-main";
  if (mode === "welcome") {
    appendText(main, "h4", "想构建什么？");
    const suggestions = document.createElement("div");
    suggestions.className = "codex-skin-suggestions";
    ["探索并理解代码", "构建新功能", "审查代码", "修复问题"].forEach((label, index) => {
      const suggestion = document.createElement("span");
      appendText(suggestion, "i", ["‹›", "✣", "✎", "⌁"][index]);
      appendText(suggestion, "b", label);
      suggestions.appendChild(suggestion);
    });
    main.appendChild(suggestions);
  } else {
    appendText(main, "small", "DreamSkin  /  src  /  main.ts", "codex-skin-crumb");
    const chat = document.createElement("div");
    chat.className = "codex-skin-chat";
    appendText(chat, "p", "帮我把主题预览改成应用后的真实界面效果。", "user");
    appendText(chat, "p", "已更新预览结构，并保留背景焦点、色板、面板透明度和边框效果。", "assistant");
    const code = appendText(chat, "pre", "+ preview: CodexInterface\n+ palette: DreamSkin\n+ state: applied");
    code.className = "codex-skin-code";
    main.appendChild(chat);
  }
  const composer = document.createElement("div");
  composer.className = "codex-skin-composer";
  appendText(composer, "span", mode === "welcome" ? "随便说点什么" : "继续对话…");
  appendText(composer, "b", "+   ⚙  自定义");
  appendText(composer, "i", "↑");
  main.appendChild(composer);
  preview.appendChild(main);
  return preview;
}

function buildCodexPreviewSwitcher(theme: CommunityTheme) {
  const frame = document.createElement("div");
  frame.className = "codex-skin-preview-frame";
  const stage = document.createElement("div");
  const controls = document.createElement("div");
  controls.className = "codex-skin-preview-controls";
  const setMode = (mode: CodexPreviewMode) => {
    stage.replaceChildren(buildCodexPreview(theme, mode, true));
    controls.querySelectorAll("button").forEach((button) => button.classList.toggle("active", button.dataset.mode === mode));
  };
  ([['welcome', '欢迎页'], ['conversation', '对话页']] as const).forEach(([mode, label]) => {
    const button = appendText(controls, "button", label);
    button.dataset.mode = mode;
    button.type = "button";
    button.addEventListener("click", () => setMode(mode));
  });
  frame.append(stage, controls);
  setMode("welcome");
  return frame;
}

function filteredCommunityThemes() {
  const query = communityQuery.trim().toLowerCase();
  if (!query) return communityThemes;
  return communityThemes.filter((theme) =>
    [theme.name, theme.author, theme.license].some((value) => String(value || "").toLowerCase().includes(query)),
  );
}

function addDetailRow(list: HTMLDListElement, label: string, value: string, mono = false) {
  const row = document.createElement("div");
  appendText(row, "dt", label);
  appendText(row, "dd", value, mono ? "mono" : undefined);
  list.appendChild(row);
}

function openCommunityTheme(theme: CommunityTheme) {
  if (!communityDetail || !communityDialog) return;
  communityDetail.replaceChildren();
  communityDetail.appendChild(buildCodexPreviewSwitcher(theme));
  const copy = document.createElement("div");
  copy.className = "community-theme-detail-copy";
  appendText(copy, "p", "DREAMSKIN COMMUNITY", "eyebrow");
  appendText(copy, "h3", theme.name);
  const list = document.createElement("dl");
  addDetailRow(list, "作者", theme.author);
  addDetailRow(list, "版本", theme.version);
  addDetailRow(list, "许可证", theme.license || "未声明");
  addDetailRow(list, "资源方式", rightsLabel(theme.rightsStatus));
  addDetailRow(list, "包大小", sizeLabel(theme.bytes));
  addDetailRow(list, "SHA-256", theme.sha256, true);
  copy.appendChild(list);
  const palette = document.createElement("div");
  palette.className = "community-theme-palette";
  Object.entries(theme.colors || {}).forEach(([key, value]) => {
    const swatch = document.createElement("span");
    swatch.style.background = value;
    swatch.title = key + " · " + value;
    palette.appendChild(swatch);
  });
  copy.appendChild(palette);
  const actions = document.createElement("div");
  actions.className = "community-theme-detail-actions";
  if (theme.installable !== false) {
    const install = document.createElement("a");
    install.className = "button";
    install.href = "osircodex://skin/install?id=" + encodeURIComponent(theme.id);
    install.textContent = "在 Manager 中安装";
    actions.appendChild(install);
  }
  const source = document.createElement("a");
  source.className = "text-link";
  source.href = theme.sourceUrl;
  source.target = "_blank";
  source.rel = "noopener";
  source.textContent = "查看 DreamSkin 来源 →";
  actions.appendChild(source);
  copy.appendChild(actions);
  appendText(copy, "p", "资源来自 DreamSkin 社区，免费提供不代表放弃版权；使用和再分发以原作者许可证为准。如有权利问题请联系下架。", "community-theme-legal");
  communityDetail.appendChild(copy);
  communityDialog.showModal();
}

function renderCommunityThemes() {
  if (!communityGrid || !communityPages || !communityCount) return;
  const items = filteredCommunityThemes();
  const pages = Math.max(1, Math.ceil(items.length / COMMUNITY_PAGE_SIZE));
  communityPage = Math.min(communityPage, pages - 1);
  const shown = items.slice(communityPage * COMMUNITY_PAGE_SIZE, (communityPage + 1) * COMMUNITY_PAGE_SIZE);
  communityCount.textContent = items.length + " 个社区主题";
  communityGrid.replaceChildren();
  shown.forEach((theme) => {
    const card = document.createElement("button");
    card.className = "community-theme-card";
    card.type = "button";
    card.appendChild(buildCodexPreview(theme, "welcome"));
    const body = document.createElement("span");
    appendText(body, "b", theme.name);
    appendText(body, "small", "@" + theme.author);
    card.appendChild(body);
    appendText(card, "em", theme.license || "未声明");
    card.addEventListener("click", () => openCommunityTheme(theme));
    communityGrid.appendChild(card);
  });
  communityPages.replaceChildren();
  const candidates = Array.from(new Set([0, pages - 1, communityPage - 1, communityPage, communityPage + 1]))
    .filter((page) => page >= 0 && page < pages)
    .sort((a, b) => a - b);
  candidates.forEach((page, index) => {
    if (index > 0 && page - candidates[index - 1] > 1) communityPages.append("…");
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = String(page + 1);
    if (page === communityPage) button.className = "active";
    button.addEventListener("click", () => {
      communityPage = page;
      renderCommunityThemes();
      document.querySelector("#themes")?.scrollIntoView();
    });
    communityPages.appendChild(button);
  });
}

if (communityGrid) {
  fetch(COMMUNITY_CATALOG)
    .then((response) => {
      if (!response.ok) throw new Error(String(response.status));
      return response.json();
    })
    .then((catalog) => {
      communityThemes = catalog.skins || [];
      renderCommunityThemes();
    })
    .catch(() => {
      if (communityCount) communityCount.textContent = "社区主题暂时无法加载";
    });
  communitySearch?.addEventListener("input", () => {
    communityQuery = communitySearch.value;
    communityPage = 0;
    renderCommunityThemes();
  });
  communityDialog?.querySelector(".community-theme-dialog-close")?.addEventListener("click", () => communityDialog.close());
  communityDialog?.addEventListener("click", (event) => {
    if (event.target === communityDialog) communityDialog.close();
  });
}

const ua = navigator.userAgent;
const isWindows = /Windows/i.test(ua);
const isMac = /Macintosh/i.test(ua) && navigator.maxTouchPoints <= 1;
const isArmMac = (() => {
  if (!isMac) return false;
  try {
    const gl = document.createElement("canvas").getContext("webgl");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    const renderer = info ? String(gl?.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "";
    return /apple|m[1-9]|arm/i.test(renderer);
  } catch {
    return false;
  }
})();
const platform = isWindows ? "windows" : isArmMac ? "mac-arm" : isMac ? "mac-intel" : null;
if (platform) {
  document.querySelector(`[data-platform="${platform}"]`)?.classList.add("recommended");
  const preferred = document.querySelector<HTMLAnchorElement>(`[data-platform="${platform}"]`);
  const hero = document.querySelector<HTMLAnchorElement>("#hero-download");
  if (hero && preferred) {
    hero.href = preferred.href;
    hero.textContent = lang === "zh" ? `下载 ${platform.startsWith("mac") ? "macOS" : "Windows"} 版` : `Download for ${platform.startsWith("mac") ? "macOS" : "Windows"}`;
  }
}
