#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";

const API_BASE = "https://api.dreamskin.cc";
const SITE_BASE = "https://dreamskin.cc";
const PAGE_SIZE = 48;
const MAX_PACKAGE_BYTES = 64 * 1024 * 1024;
const MAX_PREVIEW_BYTES = 4 * 1024 * 1024;
const argv = process.argv.slice(2);
const outputIndex = argv.indexOf("--output");
const output = resolve(outputIndex >= 0 ? argv[outputIndex + 1] : "dist/dreamskin-community");
const downloadPackages = argv.includes("--download-packages");
const execFileAsync = promisify(execFile);

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function normalizeLicense(value) {
  const raw = String(value || "").trim();
  const normalized = raw.toLowerCase().replaceAll("-", " ").replaceAll("_", " ").replace(/\\s+/g, " ");
  if (/^(mit|mit license|mit0d\\d+)$/.test(normalized)) return { label: raw || "MIT", status: "redistributable" };
  if (/^(cc0|cc by 4\\.0|cc by 4 0|cc by)$/.test(normalized)) return { label: raw, status: "redistributable" };
  if (/^(cc by nc|cc by nc sa|custom noncommercial|personal use|personal use only|private use|private local use)$/.test(normalized)) return { label: raw, status: "source-direct" };
  if (/^(all rights reserved|proprietary|unlicensed|no|unknown)$/.test(normalized) || raw.length < 2) return { label: raw || "未声明", status: "review-required" };
  return { label: raw, status: "review-required" };
}

function parseSafeCssPreviewStyle(css) {
  const source = String(css || "");
  const numeric = (property, fallback, variableName) => {
    const match = source.match(new RegExp(property + "\\s*:\\s*([^;}]+)", "i"));
    if (!match || (variableName && match[1].includes(variableName))) return fallback;
    const value = Number(match[1].match(/[0-9]+(?:\.[0-9]+)?/)?.[0]);
    return Number.isFinite(value) ? value : fallback;
  };
  const shadow = source.match(/box-shadow\s*:\s*([^;}]+)/i)?.[1]?.trim().toLowerCase();
  const blurValue = source.match(/backdrop-filter\s*:\s*[^;}]*blur\(([^)]+)\)/i)?.[1] || "";
  const blur = blurValue.includes("--ds-theme-surface-blur")
    ? 0
    : Number(blurValue.match(/[0-9]+(?:\.[0-9]+)?/)?.[0] || 0);
  const parts = [...source.matchAll(/\[data-ds-part="([a-z-]+)"\]/g)].map((match) => match[1]);
  return {
    opacity: Math.max(0.65, Math.min(1, numeric("opacity", 1, "--ds-theme-surface-opacity"))),
    blur: Math.max(0, Math.min(40, Number.isFinite(blur) ? blur : 0)),
    radius: Math.max(0, Math.min(28, numeric("border-radius", 12, "--ds-theme-surface-radius"))),
    borderAlpha: /border(?:-(?:top|right|bottom|left))?(?:-color|-width|-style)?\s*:/i.test(source) ? 0.14 : 0.08,
    shadow: shadow === "none" ? "none" : shadow ? "standard" : "soft",
    parts: [...new Set(parts)],
    hover: /\]:hover\s*\{/i.test(source),
    focusVisible: /\]:focus-visible\s*\{/i.test(source),
  };
}

async function readZipText(path, entry) {
  try {
    const { stdout } = await execFileAsync("unzip", ["-p", path, entry], {
      encoding: "utf8",
      maxBuffer: 2 * 1024 * 1024,
    });
    return stdout;
  } catch {
    return "";
  }
}

function itemToCatalog(item) {
  const license = normalizeLicense(item.license);
  const versionId = item.id;
  const version = item.version || "0.0.0";
  const sourcePackageUrl = API_BASE + "/v1/themes/" + encodeURIComponent(versionId) + "/download";
  const sourceUrl = SITE_BASE + "/themes/" + encodeURIComponent(versionId);
  return {
    id: versionId,
    themeId: item.themeId || item.id,
    name: item.name || item.slug || item.id,
    description: "DreamSkin 社区主题：" + (item.name || item.slug || item.id),
    version,
    author: item.authorDisplayName || item.authorUserId || "DreamSkin 社区作者",
    authorUserId: item.authorUserId || null,
    appearance: item.displayMeta?.appearance || null,
    license: license.label,
    rightsStatus: license.status,
    // Every reviewed DreamSkin package remains installable from its official
    // source. Only redistributable packages are copied to the OSIR mirror.
    installable: true,
    applyCompatible: Boolean(item.applyCompatible),
    category: "dreamskin-community",
    tags: ["dreamskin", "community", license.status],
    colors: item.displayMeta?.colors || {},
    art: item.displayMeta?.art || {},
    bytes: Number(item.packageBytes || 0),
    sha256: String(item.packageSha256 || ""),
    pack: license.status === "redistributable"
      ? "packs/" + versionId + "-" + version + ".codexskin"
      : "v1/themes/" + encodeURIComponent(versionId) + "/download",
    preview: "previews/" + versionId + ".jpg",
    sourceBase: license.status === "redistributable" ? "https://app.osirclaw.com/skins/dreamskin" : API_BASE,
    sourcePackageUrl,
    sourceUrl,
    source: "DreamSkin 社区",
    reviewedAt: item.reviewedAt || null,
    submittedAt: item.submittedAt || null,
    downloadCount: Number(item.downloadCount || 0),
    favoriteCount: Number(item.favoriteCount || 0),
  };
}

async function fetchJson(url) {
  const response = await fetch(url, { headers: { accept: "application/json" } });
  if (!response.ok) throw new Error(response.status + " " + url);
  return response.json();
}

async function fetchBytes(url, maxBytes) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(response.status + " " + url);
  const declared = Number(response.headers.get("content-length") || 0);
  if (declared > maxBytes) throw new Error("resource exceeds limit: " + url);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > maxBytes) throw new Error("resource exceeds limit: " + url);
  return bytes;
}

async function fetchAllThemes() {
  const first = await fetchJson(API_BASE + "/v1/themes?sort=recent&limit=" + PAGE_SIZE + "&offset=0");
  const total = Number(first.total || first.items?.length || 0);
  const items = [...(first.items || [])];
  for (let offset = PAGE_SIZE; offset < total; offset += PAGE_SIZE) {
    const page = await fetchJson(API_BASE + "/v1/themes?sort=recent&limit=" + PAGE_SIZE + "&offset=" + offset);
    items.push(...(page.items || []));
  }
  return { total, items };
}

async function main() {
  await mkdir(join(output, "metadata"), { recursive: true });
  await mkdir(join(output, "previews"), { recursive: true });
  await mkdir(join(output, "packs"), { recursive: true });
  await mkdir(join(output, "quarantine"), { recursive: true });
  const source = await fetchAllThemes();
  const unique = [...new Map(source.items.map((item) => [item.id, item])).values()];
  const catalog = unique.map(itemToCatalog);
  const failures = [];
  for (const item of catalog) {
    await writeFile(join(output, "metadata", item.id + ".json"), JSON.stringify(item, null, 2) + "\n");
    try {
      const preview = await fetchBytes(API_BASE + "/v1/themes/" + encodeURIComponent(item.id) + "/preview/thumbnail", MAX_PREVIEW_BYTES);
      await writeFile(join(output, item.preview), preview);
    } catch (error) {
      failures.push({ id: item.id, kind: "preview", error: String(error) });
    }
    if (downloadPackages) {
      try {
        const localPackage = item.rightsStatus === "redistributable"
          ? join(output, item.pack)
          : join(output, "quarantine", item.id + "-" + item.version + ".zip");
        try {
          const existing = new Uint8Array(await readFile(localPackage));
          if (!item.sha256 || sha256(existing).toLowerCase() === item.sha256.toLowerCase()) continue;
        } catch {
          // Missing or stale package; download below.
        }
        const bytes = await fetchBytes(item.sourcePackageUrl, MAX_PACKAGE_BYTES);
        const actual = sha256(bytes);
        if (item.sha256 && actual.toLowerCase() !== item.sha256.toLowerCase()) throw new Error("sha256 mismatch expected=" + item.sha256 + " actual=" + actual);
        await mkdir(dirname(localPackage), { recursive: true });
        await writeFile(localPackage, bytes);
      } catch (error) {
        failures.push({ id: item.id, kind: "package", error: String(error) });
      }
    }
  }
  for (const item of catalog) {
    const packagePath = item.rightsStatus === "redistributable"
      ? join(output, item.pack)
      : join(output, "quarantine", item.id + "-" + item.version + ".zip");
    item.previewStyle = parseSafeCssPreviewStyle(await readZipText(packagePath, "theme.css"));
    await writeFile(join(output, "metadata", item.id + ".json"), JSON.stringify(item, null, 2) + "\n");
  }
  const manifest = {
    schemaVersion: 2,
    generatedAt: new Date().toISOString(),
    source: SITE_BASE,
    apiSource: API_BASE,
    sourceCount: source.total,
    itemCount: catalog.length,
    rights: {
      redistributable: catalog.filter((item) => item.rightsStatus === "redistributable").length,
      sourceDirect: catalog.filter((item) => item.rightsStatus === "source-direct").length,
      reviewRequired: catalog.filter((item) => item.rightsStatus === "review-required").length,
    },
    mirrorLayout: "dreamskin/v1/{previews,packages,metadata}",
    notice: "来源于 DreamSkin 社区；许可证和再分发范围以每套主题的原始声明为准。如有权利问题，请联系下架。",
    skins: catalog,
    failures,
  };
  await writeFile(join(output, "index.json"), JSON.stringify(manifest, null, 2) + "\n");
  console.log(JSON.stringify({ output, total: source.total, items: catalog.length, rights: manifest.rights, failures: failures.length, downloadedPackages: downloadPackages }, null, 2));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
