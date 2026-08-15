import "./styles/redesign.css";

type Lang = "zh" | "en";
let lang: Lang = (localStorage.getItem("cam-site-lang") as Lang) || "zh";

const applyLang = (next: Lang) => {
  lang = next;
  localStorage.setItem("cam-site-lang", next);
  document.documentElement.lang = next === "zh" ? "zh-CN" : "en";
  document.querySelectorAll<HTMLElement>("[data-zh][data-en]").forEach((el) => {
    el.innerHTML = el.dataset[next] || "";
  });
  const button = document.querySelector<HTMLButtonElement>("#lang-switch");
  if (button) button.textContent = next === "zh" ? "EN" : "中文";
};

applyLang(lang);
document.querySelector("#lang-switch")?.addEventListener("click", () => applyLang(lang === "zh" ? "en" : "zh"));

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
