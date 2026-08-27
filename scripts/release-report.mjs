#!/usr/bin/env node

import { execFile } from "node:child_process";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { promisify } from "node:util";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
export const DEFAULT_MIRROR_BASE = "https://app.osirclaw.com/manager";
export const PLATFORM_FILES = {
  "windows-x86_64": (version) => `CodexManager_${version}_x64-setup.exe`,
  "windows-aarch64": (version) => `CodexManager_${version}_arm64-setup.exe`,
  "darwin-aarch64": () => "CodexManager_aarch64.dmg",
  "darwin-x86_64": () => "CodexManager_x86_64.dmg",
};

const cleanUrl = (value) => {
  if (!value) return "";
  try {
    const url = new URL(String(value));
    url.search = "";
    url.hash = "";
    return url.toString();
  } catch {
    return String(value).split(/[?#]/, 1)[0];
  }
};

const cell = (value) => String(value ?? "").replace(/\s+/g, " ").replaceAll("|", "\\|").replaceAll("`", "'").slice(0, 500);
const basenameFromUrl = (value) => {
  try { return decodeURIComponent(new URL(value).pathname.split("/").pop() || ""); }
  catch { return ""; }
};
async function readJson(path, label = path) {
  try { return JSON.parse(await readFile(path, "utf8")); }
  catch (error) { throw new Error(`无法读取${label}: ${error instanceof Error ? error.message : String(error)}`); }
}

export async function collectGitFacts(cwd = REPO_ROOT, run = execFileAsync) {
  const git = async (...args) => (await run("git", ["-C", cwd, ...args], { encoding: "utf8" })).stdout.trim();
  const [sha, branch, status] = await Promise.all([
    git("rev-parse", "HEAD"),
    git("branch", "--show-current"),
    git("status", "--short"),
  ]);
  return { sha, branch, clean: status.length === 0, status: status ? status.split("\n") : [] };
}

export async function collectReleaseJson(tag, run = execFileAsync, repo = "ousir0/osir-codex-manager") {
  const result = await run("gh", ["api", `repos/${repo}/releases/tags/${tag}`], { encoding: "utf8" });
  return JSON.parse(result.stdout);
}

export async function collectReleaseCommitSha(tag, run = execFileAsync, repo = "ousir0/osir-codex-manager") {
  try {
    const result = await run("gh", ["api", `repos/${repo}/commits/${tag}`], { encoding: "utf8" });
    return JSON.parse(result.stdout).sha || "";
  } catch {
    return "";
  }
}

export async function collectCiFacts({ sha, tag }, run = execFileAsync, repo = "ousir0/osir-codex-manager") {
  try {
    const result = await run("gh", ["run", "list", "--repo", repo, "--limit", "30", "--json", "name,status,conclusion,headSha,event,url,createdAt"], { encoding: "utf8" });
    const runs = JSON.parse(result.stdout);
    const matching = runs.filter((item) => item.headSha === sha);
    const completed = matching.filter((item) => item.status === "completed");
    const failed = completed.find((item) => item.conclusion !== "success");
    return {
      status: failed ? "失败" : completed.length ? "通过" : "未找到",
      runs: matching.slice(0, 8),
      tag,
    };
  } catch (error) {
    return { status: "读取失败", error: error instanceof Error ? error.message : String(error), runs: [] };
  }
}

async function headUrl(url, fetchImpl = globalThis.fetch) {
  if (!fetchImpl) return { status: "not-run", size: null };
  try {
    const response = await fetchImpl(url, { method: "HEAD", redirect: "follow" });
    const length = response.headers?.get?.("content-length");
    return { status: response.status, size: length ? Number(length) : null, url: cleanUrl(response.url || url) };
  } catch (error) {
    return { status: "error", size: null, error: error instanceof Error ? error.message : String(error), url: cleanUrl(url) };
  }
}

export async function buildReleaseReport({ version, tag = `v${version}`, git, release, latest, mirrorBase = DEFAULT_MIRROR_BASE, onlineChecks = {}, ci, generatedAt = new Date().toISOString() }) {
  const normalizedVersion = String(version).replace(/^v/, "");
  const assets = new Map((release?.assets || []).map((asset) => [asset.name, asset]));
  const platforms = latest?.platforms || {};
  const rows = [];
  rows.push(`# Codex Manager ${tag} 发布验收报告`, "", `生成时间：${generatedAt}`, `版本：${tag}`, `发布源码 SHA：${git?.releaseSha || git?.sha || "未知"}`, `当前工作区 SHA：${git?.sha || "未知"}`, `分支：${git?.branch || "未知"}`, `工作区：${git?.clean ? "干净" : "有遗留变更"}`, "");
  rows.push("## 发布状态", "", "| 项目 | 状态 | 说明 |", "| --- | --- | --- |");
  rows.push(`| GitHub Release | ${release ? (release.draft ? "失败" : "已发布") : "未读取"} | ${cleanUrl(release?.html_url || "")} |`);
  rows.push(`| Release immutable | ${(release?.isImmutable === true || release?.immutable === true) ? "通过" : release ? "需确认" : "未读取"} | 不覆盖历史版本 |`);
  rows.push(`| 线上 latest.json | ${latest?.version === normalizedVersion ? "通过" : latest ? "版本不一致" : "未读取"} | ${cleanUrl(`${mirrorBase}/latest.json`)} |`);
  rows.push("");
  rows.push("## 四平台资产与线上下载", "", "| 平台 | 文件 | Release | 大小 | SHA-256 | 线上 HTTP | 线上大小 | 线上 URL |", "| --- | --- | --- | ---: | --- | ---: | ---: | --- |");
  for (const [platform, filename] of Object.entries(PLATFORM_FILES)) {
    const desktopName = filename(normalizedVersion);
    const manifest = platforms[platform];
    const updaterName = basenameFromUrl(manifest?.url) || desktopName;
    const asset = assets.get(updaterName) || assets.get(desktopName);
    const supplemental = updaterName !== desktopName && assets.has(desktopName) ? `；桌面包 ${desktopName} ${assets.get(desktopName).size} bytes` : "";
    const check = onlineChecks[platform] || {};
    rows.push(`| ${platform} | ${updaterName}${supplemental} | ${asset ? "存在" : "缺失"} | ${asset?.size ?? "—"} | ${cell(asset?.digest?.replace(/^sha256:/, "") || manifest?.sha256 || "—")} | ${check.status ?? "未检查"} | ${check.size ?? "—"} | ${cleanUrl(manifest?.url || `${mirrorBase}/${normalizedVersion}/${encodeURIComponent(updaterName)}`)} |`);
  }
  rows.push("", "## CI、镜像与用户验收", "", "| 检查项 | 结果 |", "| --- | --- |");
  rows.push(`| CI | ${cell(release?.ci_status || onlineChecks.ci || "请回看 GitHub Actions")} |`);
  rows.push(`| Release | ${release ? "已读取" : "未读取"} |`);
  rows.push(`| 镜像 | ${cell(onlineChecks.mirror || "请确认 current 与版本目录") } |`);
  if (ci?.runs?.length) {
    rows.push("", "### CI / Release 工作流明细", "", "| 工作流 | 结论 | SHA | 链接 |", "| --- | --- | --- | --- |");
    for (const run of ci.runs) rows.push(`| ${cell(run.name)} | ${cell(run.conclusion || run.status)} | ${cell(run.headSha)} | ${cleanUrl(run.url)} |`);
  }
  rows.push("", "### 用户一键更新验收", "", "- [ ] 旧版客户端发现新版本", "- [ ] 用户点击“立即更新”", "- [ ] 下载与签名校验通过", "- [ ] 客户端自动重启", `- [ ] 关于页显示 ${tag}`, "");
  rows.push("### 异常与回滚", "", "- 异常：无（如有请补充）", "- 回滚：未执行（如需回滚，保留旧版本目录并原子切换 current）", "");
  if (git?.status?.length) rows.push("### 工作区遗留", "", ...git.status.map((item) => `- ${cell(item)}`), "");
  rows.push("> 本报告只记录验收事实；不会保存带 query 的预签名 URL、API Key 或私钥。", "");
  return rows.join("\n");
}

export async function generateReleaseReport({ version, output, releaseJson, latestJson, mirrorBase = DEFAULT_MIRROR_BASE, repo = "ousir0/osir-codex-manager", cwd = REPO_ROOT, fetchImpl = globalThis.fetch, run = execFileAsync }) {
  const tag = version.startsWith("v") ? version : `v${version}`;
  const normalizedVersion = tag.slice(1);
  const git = await collectGitFacts(cwd, run);
  const release = releaseJson ? await readJson(releaseJson, "Release JSON") : await collectReleaseJson(tag, run, repo);
  const releaseSha = releaseJson ? "" : await collectReleaseCommitSha(tag, run, repo);
  const ci = await collectCiFacts({ sha: releaseSha || git.sha, tag }, run, repo);
  const latest = latestJson ? await readJson(latestJson, "latest.json") : await (async () => {
    const response = await fetchImpl(`${mirrorBase}/latest.json`, { redirect: "follow" });
    if (!response.ok) throw new Error(`latest.json HTTP ${response.status}`);
    return response.json();
  })();
  const onlineChecks = {};
  for (const [platform, getName] of Object.entries(PLATFORM_FILES)) {
    const desktopName = getName(normalizedVersion);
    const manifestUrl = latest?.platforms?.[platform]?.url;
    const name = basenameFromUrl(manifestUrl) || desktopName;
    onlineChecks[platform] = await headUrl(manifestUrl || `${mirrorBase}/${normalizedVersion}/${encodeURIComponent(name)}`, fetchImpl);
  }
  const report = await buildReleaseReport({ version: normalizedVersion, tag, git: { ...git, releaseSha }, release: { ...release, ci_status: ci.status }, latest, mirrorBase, onlineChecks: { ...onlineChecks, ci: ci.status, mirror: latest?.version === normalizedVersion ? "latest.json 与目标版本一致" : "版本不一致" }, ci });
  const target = resolve(output || join(REPO_ROOT, "docs", "release-reports", `${tag}.md`));
  await mkdir(dirname(target), { recursive: true });
  await writeFile(target, report);
  return { target, report };
}

function parseArgs(args) {
  const [version, ...rest] = args;
  if (!version || version.startsWith("-")) throw new Error("用法：node scripts/release-report.mjs vX.Y.Z [--output path] [--release-json path] [--latest-json path] [--mirror-base URL]");
  const options = { version };
  for (let i = 0; i < rest.length; i += 1) {
    const key = rest[i];
    const value = rest[++i];
    if (!value || !["--output", "--release-json", "--latest-json", "--mirror-base"].includes(key)) throw new Error(`未知参数：${key}`);
    options[{ "--output": "output", "--release-json": "releaseJson", "--latest-json": "latestJson", "--mirror-base": "mirrorBase" }[key]] = value;
  }
  return options;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    const result = await generateReleaseReport(parseArgs(process.argv.slice(2)));
    console.log(`已生成发布验收报告：${result.target}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
