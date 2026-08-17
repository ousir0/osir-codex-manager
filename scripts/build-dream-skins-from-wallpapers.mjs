#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const DEFAULT_SOURCE = String.raw`C:\Users\wei\AppData\Roaming\Hao Wallpaper\WallpaperDownloads`;
const DEFAULT_SKIP = "wallpaper-local_1779096458597_lgwejv";
const DEFAULT_OUTPUT = path.resolve("dist", `dream-skins-${new Date().toISOString().slice(0, 10).replaceAll("-", "")}`);
const CLIENT_VERSION = "1.5.14";
const IMAGE_WIDTH = 2560;
const IMAGE_HEIGHT = 1440;
const IMAGE_QUALITY = "86";
const WINDOWS_TAR = "/mnt/c/Windows/System32/tar.exe";

const THEME_NAMES = [
  "Ink Wanderer",
  "Quiet Resolve",
  "Skybound Motion",
  "Lunar Edge",
  "Blue Stroke",
  "Lakeside Evening",
  "Window at Night",
  "Pixel Workshop",
  "Neon Ridge",
  "Sea Wind",
  "Ashen Machine",
  "Red Signal",
  "Blue Guardians",
  "Mountain River",
  "Sunset Mark",
  "Cosmic Pair",
  "Open Field",
  "Red Frequency",
  "Monochrome Strings",
  "Skyline Story",
  "Forest Afternoon",
];

const WARNINGS = new Map([
  ["Quiet Resolve", "The source wallpaper contains baked-in Chinese text."],
  ["Window at Night", "The source wallpaper contains a baked-in Windows logo."],
  ["Sunset Mark", "The source wallpaper contains a baked-in emblem."],
  ["Skyline Story", "The source wallpaper contains a recognizable animated character."],
  ["Forest Afternoon", "The source wallpaper contains recognizable animated characters."],
]);

const SAFE_CSS = `[data-ds-part="root"] {
  color: var(--ds-theme-color-text);
}

[data-ds-part="sidebar"] {
  background-color: var(--ds-theme-color-panel);
  border-right-color: var(--ds-theme-color-line);
}

[data-ds-part="composer"] {
  background-color: var(--ds-theme-color-panel-alt);
  border-color: var(--ds-theme-color-line);
}

[data-ds-part="composer"]:focus-visible {
  border-color: var(--ds-theme-color-accent);
}
`;

function mapWindowsPath(input) {
  if (process.platform !== "win32" && input && /^([A-Za-z]):[\\/]/u.test(input)) {
    const drive = input[0].toLowerCase();
    return `/mnt/${drive}/${input.slice(3).replaceAll("\\", "/")}`;
  }
  return input;
}

function parseArgs(argv) {
  const values = { source: DEFAULT_SOURCE, output: DEFAULT_OUTPUT, skip: DEFAULT_SKIP };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    if (!(key in values)) throw new Error(`Unknown option: ${token}`);
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for ${token}`);
    values[key] = value;
    index += 1;
  }
  values.source = mapWindowsPath(values.source);
  values.output = path.resolve(values.output);
  return values;
}

async function findFirstPng(directory) {
  const entries = await fs.readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isFile() && /\.png$/iu.test(entry.name)) files.push(fullPath);
    if (entry.isDirectory()) {
      const nested = await findFirstPng(fullPath);
      if (nested) files.push(nested);
    }
  }
  return files[0] ?? null;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function rgbToHsl(red, green, blue) {
  const r = red / 255;
  const g = green / 255;
  const b = blue / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l: lightness };
  const delta = max - min;
  const saturation = lightness > 0.5 ? delta / (2 - max - min) : delta / (max + min);
  let hue;
  if (max === r) hue = (g - b) / delta + (g < b ? 6 : 0);
  else if (max === g) hue = (b - r) / delta + 2;
  else hue = (r - g) / delta + 4;
  return { h: hue / 6, s: saturation, l: lightness };
}

function hslToHex(hue, saturation, lightness) {
  const h = ((hue % 1) + 1) % 1;
  const s = clamp(saturation, 0, 1);
  const l = clamp(lightness, 0, 1);
  if (s === 0) {
    const value = Math.round(l * 255).toString(16).padStart(2, "0");
    return `#${value}${value}${value}`;
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const channel = (t) => {
    let value = t;
    if (value < 0) value += 1;
    if (value > 1) value -= 1;
    if (value < 1 / 6) value = p + (q - p) * 6 * value;
    else if (value < 1 / 2) value = q;
    else if (value < 2 / 3) value = p + (q - p) * (2 / 3 - value) * 6;
    else value = p;
    return Math.round(value * 255).toString(16).padStart(2, "0");
  };
  return `#${channel(h + 1 / 3)}${channel(h)}${channel(h - 1 / 3)}`;
}

function parseHistogram(imagePath) {
  let output;
  try {
    output = execFileSync("convert", [
      imagePath,
      "-background", "#11151b",
      "-alpha", "remove",
      "-resize", "96x96",
      "-colors", "12",
      "-format", "%c",
      "histogram:info:-",
    ], { encoding: "utf8" });
  } catch {
    process.stderr.write(`Color analysis failed for ${path.basename(path.dirname(imagePath))}; using OSIR blue fallback.\n`);
    return [];
  }
  return output.split("\n").flatMap((line) => {
    const match = line.match(/\s*(\d+):.*#([0-9a-f]{6})/iu);
    if (!match) return [];
    const hex = match[2];
    const rgb = [0, 2, 4].map((index) => Number.parseInt(hex.slice(index, index + 2), 16));
    return [{ count: Number(match[1]), rgb, hsl: rgbToHsl(...rgb) }];
  });
}

function makeColors(imagePath) {
  const colors = parseHistogram(imagePath);
  const dominant = colors.reduce((best, current) => current.count > (best?.count ?? -1) ? current : best, null);
  const ranked = [...colors].sort((a, b) => {
    const score = (entry) => entry.hsl.s * (0.4 + 0.6 * Math.sqrt(entry.count / Math.max(1, dominant?.count ?? entry.count)));
    return score(b) - score(a);
  });
  const selected = ranked.find((entry) => entry.hsl.s >= 0.18) ?? ranked[0] ?? {
    hsl: { h: 0.59, s: 0.92, l: 0.70 },
  };
  const hue = selected.hsl.h;
  const saturation = Math.max(0.58, selected.hsl.s);
  const accent = hslToHex(hue, saturation, clamp(selected.hsl.l, 0.48, 0.68));
  const accentAlt = hslToHex(hue, Math.min(0.9, saturation + 0.08), clamp(selected.hsl.l + 0.14, 0.62, 0.82));
  const secondary = hslToHex(hue + 0.46, Math.max(0.46, saturation * 0.78), 0.58);
  const highlight = hslToHex(hue + 0.88, Math.max(0.52, saturation * 0.84), 0.60);
  const background = hslToHex(hue, Math.min(0.28, saturation * 0.32), 0.075);
  const panel = hslToHex(hue, Math.min(0.32, saturation * 0.38), 0.105);
  const panelAlt = hslToHex(hue, Math.min(0.38, saturation * 0.45), 0.145);
  const [r, g, b] = [accent.slice(1, 3), accent.slice(3, 5), accent.slice(5, 7)].map((value) => Number.parseInt(value, 16));
  return {
    background,
    panel,
    panelAlt,
    accent,
    accentAlt,
    secondary,
    highlight,
    text: "#f3f7fb",
    muted: "#aeb9c5",
    line: `rgba(${r}, ${g}, ${b}, .28)`,
  };
}

function getImageDimensions(imagePath) {
  const output = execFileSync("identify", ["-format", "%wx%h", imagePath], { encoding: "utf8" }).trim();
  const match = output.match(/^(\d+)x(\d+)$/u);
  return match ? { width: Number(match[1]), height: Number(match[2]) } : null;
}

function requireBytes(filePath) {
  return readFileSync(filePath);
}

function fileEntry(name, mediaType, filePath) {
  const bytes = requireBytes(filePath);
  return { path: name, mediaType, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}

async function writeJson(filePath, value) {
  await fs.writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function createArchive(archive, packageDirectory) {
  const tarCommand = existsSync(WINDOWS_TAR) ? WINDOWS_TAR : process.platform === "win32" ? "tar.exe" : null;
  if (tarCommand) {
    execFileSync(tarCommand, [
      "-a", "-c", "-f", archive,
      "-C", packageDirectory,
      "manifest.json", "theme.json", "theme.css", "background.webp",
    ], { stdio: "inherit" });
    return;
  }
  execFileSync("zip", ["-q", "-X", archive, "manifest.json", "theme.json", "theme.css", "background.webp"], {
    cwd: packageDirectory,
    stdio: "inherit",
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const source = path.resolve(args.source);
  const sourceStat = await fs.stat(source).catch(() => null);
  if (!sourceStat?.isDirectory()) {
    throw new Error(`Wallpaper directory does not exist: ${source}`);
  }
  await fs.mkdir(args.output, { recursive: true });

  const folders = (await fs.readdir(source, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory() && entry.name !== args.skip)
    .sort((a, b) => a.name.localeCompare(b.name));
  if (!folders.length) throw new Error("No wallpaper folders found");

  const generated = [];
  for (let index = 0; index < folders.length; index += 1) {
    const folder = folders[index];
    const sourceImage = await findFirstPng(path.join(source, folder.name));
    if (!sourceImage) {
      generated.push({ sourceFolder: folder.name, status: "skipped", reason: "No PNG found" });
      continue;
    }
    const id = `osir-${String(index + 1).padStart(2, "0")}`;
    const name = THEME_NAMES[index] ?? `OSIR ${String(index + 1).padStart(2, "0")}`;
    const packageDirectory = path.join(args.output, id);
    await fs.mkdir(packageDirectory, { recursive: true });
    const imagePath = path.join(packageDirectory, "background.webp");
    execFileSync("ffmpeg", [
      "-hide_banner", "-loglevel", "error", "-y",
      "-i", sourceImage,
      "-vf", `scale=${IMAGE_WIDTH}:${IMAGE_HEIGHT}:force_original_aspect_ratio=increase,crop=${IMAGE_WIDTH}:${IMAGE_HEIGHT}`,
      "-frames:v", "1",
      "-c:v", "libwebp",
      "-quality", IMAGE_QUALITY,
      "-compression_level", "6",
      imagePath,
    ], { stdio: "inherit" });

    await fs.writeFile(path.join(packageDirectory, "theme.css"), SAFE_CSS, "utf8");
    const theme = {
      schemaVersion: 1,
      id,
      name,
      image: "background.webp",
      appearance: "auto",
      art: { taskMode: "ambient" },
      colors: makeColors(imagePath),
    };
    await writeJson(path.join(packageDirectory, "theme.json"), theme);
    const files = [
      fileEntry("theme.json", "application/json", path.join(packageDirectory, "theme.json")),
      fileEntry("background.webp", "image/webp", imagePath),
      fileEntry("theme.css", "text/css", path.join(packageDirectory, "theme.css")),
    ];
    const manifest = {
      packageVersion: 1,
      themeId: id,
      version: "1.0.0",
      skinApiVersion: 1,
      minClientVersion: "1.5.0",
      platforms: ["windows"],
      capabilities: ["background", "tokens", "safe-css"],
      publisher: { id: "osir", displayName: "OSIR" },
      license: "Proprietary",
      provenance: {
        aiGenerated: false,
        summary: "User-provided local wallpaper normalized for Dream Skin preview.",
      },
      files,
      createdAt: new Date().toISOString(),
    };
    await writeJson(path.join(packageDirectory, "manifest.json"), manifest);

    const archive = path.join(args.output, `${id}.zip`);
    createArchive(archive, packageDirectory);
    const sourceDimensions = getImageDimensions(sourceImage);
    const packagedBytes = (await fs.stat(imagePath)).size;
    generated.push({
      id,
      name,
      sourceFolder: folder.name,
      sourceImage: path.basename(sourceImage),
      sourceDimensions,
      packagedImageBytes: packagedBytes,
      archive: `${id}.zip`,
      status: "generated",
      warning: WARNINGS.get(name) ?? null,
    });
    process.stdout.write(`${id}: ${name} <- ${folder.name} (${Math.round(packagedBytes / 1024)} KiB)\n`);
  }

  await writeJson(path.join(args.output, "index.json"), {
    generatedAt: new Date().toISOString(),
    source: "Windows Hao Wallpaper WallpaperDownloads",
    skippedFolder: args.skip,
    imageSize: `${IMAGE_WIDTH}x${IMAGE_HEIGHT}`,
    clientVersion: CLIENT_VERSION,
    branding: "Brand and API address intentionally omitted from skin packages; keep them in the manager.",
    themes: generated,
  });
  await fs.writeFile(path.join(args.output, "README.txt"), [
    "OSIR Dream Skin candidate packs",
    "",
    "Each ZIP is a Windows Dream Skin package generated from one user-provided PNG.",
    "Brand and API address are intentionally omitted; keep those in the manager UI.",
    "Images were normalized to 2560x1440 WebP for local preview.",
    "Some source wallpapers contain baked text, logos, or recognizable characters.",
    "Verify image rights before sharing these packages outside your own machine.",
    "",
  ].join("\n"), "utf8");
}

main().catch((error) => {
  process.stderr.write(`${error?.stack ?? error}\n`);
  process.exitCode = 1;
});
