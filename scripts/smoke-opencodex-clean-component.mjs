#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const target = process.argv[2] || "darwin-arm64";
const manifestUrl = process.env.OPENCODEX_COMPONENT_MANIFEST_URL ||
  "https://app.osirclaw.com/components/opencodex/index.json";
const root = mkdtempSync(join(tmpdir(), "codex-manager-clean-component-"));
const home = join(root, "home");
const codexHome = join(root, ".codex");
const archive = join(root, "component.zip");
const extract = join(root, "component");

try {
  mkdirSync(home, { recursive: true });
  mkdirSync(codexHome, { recursive: true });
  const manifestResponse = await fetch(manifestUrl);
  if (!manifestResponse.ok) throw new Error(`manifest HTTP ${manifestResponse.status}`);
  const manifest = await manifestResponse.json();
  const component = manifest?.targets?.[target];
  if (!component) throw new Error(`manifest has no ${target} target`);

  const artifactResponse = await fetch(component.url);
  if (!artifactResponse.ok) throw new Error(`artifact HTTP ${artifactResponse.status}`);
  const artifact = Buffer.from(await artifactResponse.arrayBuffer());
  const sha256 = createHash("sha256").update(artifact).digest("hex");
  if (sha256 !== component.sha256) {
    throw new Error(`sha256 mismatch: ${sha256} != ${component.sha256}`);
  }
  writeFileSync(archive, artifact);
  execFileSync("unzip", ["-q", archive, "-d", extract]);

  const windows = target.startsWith("windows-");
  const node = join(extract, windows ? "runtime/node.exe" : "runtime/bin/node");
  const launcher = join(extract, "opencodex/node_modules/@bitkyc08/opencodex/bin/ocx.mjs");
  const license = join(extract, "OPENCODEX-LICENSE");
  if (!existsSync(node) || !existsSync(launcher) || !existsSync(license)) {
    throw new Error("component is missing its runtime, launcher, or license");
  }

  const childEnv = {
    HOME: home,
    USERPROFILE: home,
    CODEX_HOME: codexHome,
    PATH: process.platform === "win32" ? "C:\\Windows\\System32" : "/usr/bin:/bin",
  };
  const version = execFileSync(node, [launcher, "--version"], { encoding: "utf8", env: childEnv }).trim();
  if (!version.includes(String(manifest.version))) {
    throw new Error(`unexpected OpenCodex version: ${version}`);
  }

  const candidate = join(root, "candidate.json");
  writeFileSync(candidate, JSON.stringify({ providers: {}, customModels: [] }));
  execFileSync(node, [launcher, "config", "validate", candidate], { encoding: "utf8", env: childEnv });

  console.log(JSON.stringify({
    ok: true,
    target,
    version,
    sha256,
    npmOnChildPath: false,
    tempHome: home,
  }, null, 2));
} finally {
  rmSync(root, { recursive: true, force: true });
}
