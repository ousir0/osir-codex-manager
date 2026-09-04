#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const target = process.argv[2] || "darwin-arm64";
const localArchive = process.argv[3] || "";
const manifestUrl = process.env.OPENCODEX_COMPONENT_MANIFEST_URL ||
  "https://app.osirclaw.com/components/opencodex/index.json";
const root = mkdtempSync(join(tmpdir(), "codex-manager-clean-component-"));
const home = join(root, "home");
const codexHome = join(root, ".codex");
const archive = join(root, "component.zip");
const extract = join(root, "component");

function peMachine(bytes) {
  if (bytes.length < 0x40 || bytes.toString("ascii", 0, 2) !== "MZ") return null;
  const peOffset = bytes.readUInt32LE(0x3c);
  if (peOffset + 6 > bytes.length || bytes.toString("ascii", peOffset, peOffset + 4) !== "PE\0\0") return null;
  return bytes.readUInt16LE(peOffset + 4);
}

function expectedMachine(target) {
  return target === "windows-arm64" ? 0xaa64 : 0x8664;
}

function isNativeTarget(target) {
  const nativeTarget = {
    darwin: process.arch === "arm64" ? "darwin-arm64" : "darwin-x64",
    linux: process.arch === "arm64" ? "linux-arm64" : "linux-x64",
    win32: process.arch === "arm64" ? "windows-arm64" : "windows-x64",
  }[process.platform];
  return nativeTarget === target;
}

try {
  mkdirSync(home, { recursive: true });
  mkdirSync(codexHome, { recursive: true });
  let manifest;
  let component;
  let artifact;
  if (localArchive) {
    artifact = readFileSync(localArchive);
    manifest = { version: "local" };
    component = { sha256: createHash("sha256").update(artifact).digest("hex") };
  } else {
    const manifestResponse = await fetch(manifestUrl);
    if (!manifestResponse.ok) throw new Error(`manifest HTTP ${manifestResponse.status}`);
    manifest = await manifestResponse.json();
    component = manifest?.targets?.[target];
    if (!component) throw new Error(`manifest has no ${target} target`);

    const artifactResponse = await fetch(component.url);
    if (!artifactResponse.ok) throw new Error(`artifact HTTP ${artifactResponse.status}`);
    artifact = Buffer.from(await artifactResponse.arrayBuffer());
  }
  const sha256 = createHash("sha256").update(artifact).digest("hex");
  if (sha256 !== component.sha256) {
    throw new Error(`sha256 mismatch: ${sha256} != ${component.sha256}`);
  }
  writeFileSync(archive, artifact);
  execFileSync("unzip", ["-q", archive, "-d", extract]);

  const windows = target.startsWith("windows-");
  const node = join(extract, windows ? "runtime/node.exe" : "runtime/bin/node");
  const launcher = join(extract, "opencodex/node_modules/@bitkyc08/opencodex/bin/ocx.mjs");
  const bun = join(extract, "opencodex/node_modules/bun/bin", windows ? "bun.exe" : "bun");
  const license = join(extract, "OPENCODEX-LICENSE");
  if (!existsSync(node) || !existsSync(launcher) || !existsSync(bun) || !existsSync(license)) {
    throw new Error("component is missing its Node runtime, Bun runtime, launcher, or license");
  }

  const machine = windows ? peMachine(readFileSync(bun)) : null;
  if (windows && machine !== expectedMachine(target)) {
    throw new Error("Bun runtime has the wrong executable format for " + target);
  }

  const childEnv = {
    HOME: home,
    USERPROFILE: home,
    CODEX_HOME: codexHome,
    PATH: process.platform === "win32" ? "C:\\Windows\\System32" : "/usr/bin:/bin",
  };
  if (!isNativeTarget(target)) {
    console.log(JSON.stringify({
      ok: true,
      target,
      sha256,
      executionCheck: "skipped-on-non-native-host",
    }, null, 2));
    process.exit(0);
  }
  const version = execFileSync(node, [launcher, "--version"], { encoding: "utf8", env: childEnv }).trim();
  if (!localArchive && !version.includes(String(manifest.version))) {
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
    bun,
    tempHome: home,
  }, null, 2));
} finally {
  rmSync(root, { recursive: true, force: true });
}
