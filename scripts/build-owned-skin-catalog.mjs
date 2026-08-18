#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const args = process.argv.slice(2);
const valueFor = (flag) => {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : null;
};
const source = path.resolve(valueFor("--source") || "");
const output = path.resolve(valueFor("--output") || path.join(process.cwd(), "skins"));

if (!valueFor("--source")) {
  console.error("usage: node scripts/build-owned-skin-catalog.mjs --source <source-repo> [--output skins]");
  process.exit(2);
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const writeJson = (file, value) => fs.writeFile(file, `${JSON.stringify(value, null, 2)}\n`);

function packageTheme(directory, target) {
  execFileSync("zip", ["-q", "-X", "-r", target, "."], { cwd: directory });
}

function ownedId(id) {
  if (!/^awai-[0-9]{2}$/u.test(id)) throw new Error(`unexpected source theme id: ${id}`); // ownership-audit: allow-legacy
  return id.replace(/^awai-/u, "codex-"); // ownership-audit: allow-legacy
}

async function main() {
  const index = JSON.parse(await fs.readFile(path.join(source, "index.json"), "utf8"));
  if (!Array.isArray(index.skins) || index.skins.length === 0) {
    throw new Error("source catalog has no skins");
  }

  for (const generated of ["themes", "packs", "previews"]) {
    await fs.rm(path.join(output, generated), { recursive: true, force: true });
  }
  await fs.rm(path.join(output, "index.json"), { force: true });
  await fs.mkdir(path.join(output, "themes"), { recursive: true });
  await fs.mkdir(path.join(output, "packs"), { recursive: true });
  await fs.mkdir(path.join(output, "previews"), { recursive: true });
  await fs.copyFile(path.join(source, "LICENSE"), path.join(output, "LICENSE"));

  const skins = [];
  for (const entry of index.skins) {
    const id = ownedId(entry.id);
    const version = entry.version || "1.0.0";
    const sourceTheme = path.join(source, "themes", entry.id);
    const themeDir = path.join(output, "themes", id);
    await fs.cp(sourceTheme, themeDir, { recursive: true });

    const themePath = path.join(themeDir, "theme.json");
    const theme = JSON.parse(await fs.readFile(themePath, "utf8"));
    theme.id = id;
    theme.author = "Codex Manager";
    theme.description = String(theme.description || entry.description || theme.name || id)
      .replaceAll("AWAI", "Codex Manager") // ownership-audit: allow-legacy
      .replaceAll("awai-", "codex-"); // ownership-audit: allow-legacy
    await writeJson(themePath, theme);

    const previewName = `${id}.webp`;
    const sourcePreview = path.join(source, "previews", `${entry.id}.webp`);
    const fallbackPreview = path.join(themeDir, "previews", "home.webp");
    await fs.copyFile(
      await fs.stat(sourcePreview).then(() => sourcePreview).catch(() => fallbackPreview),
      path.join(output, "previews", previewName),
    );

    const packName = `${id}-${version}.codexskin`;
    const packPath = path.join(output, "packs", packName);
    packageTheme(themeDir, packPath);
    const pack = await fs.readFile(packPath);
    skins.push({
      id,
      name: theme.name || entry.name || id,
      description: theme.description,
      version,
      author: "Codex Manager",
      appearance: theme.appearance || entry.appearance || "dual",
      license: theme.license || entry.license || "personal-use",
      category: theme.category || entry.category || "wallpaper",
      tags: theme.tags || entry.tags || [],
      codexVerified: theme.codexVerified ?? null,
      bytes: pack.length,
      sha256: sha256(pack),
      pack: `packs/${packName}`,
      preview: `previews/${previewName}`,
    });
  }

  skins.sort((left, right) => left.id.localeCompare(right.id));
  await writeJson(path.join(output, "index.json"), {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    source: "https://github.com/ousir0/osir-codex-manager/tree/main/skins",
    sourceBuilder: "scripts/build-owned-skin-catalog.mjs",
    branding: "Theme packages contain visual styles only; Codex Manager owns runtime configuration.",
    skins,
  });
  console.log(JSON.stringify({ output, skins: skins.length }, null, 2));
}

main().catch((error) => {
  console.error(error?.stack || error);
  process.exitCode = 1;
});
