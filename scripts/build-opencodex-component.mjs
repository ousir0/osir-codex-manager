#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const value = (flag, fallback = "") => {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] || fallback : fallback;
};

const version = value("--version", "2.22.0");
const target = value("--target");
const output = resolve(value("--output", `dist/opencodex-${target}.zip`));
const nodeVersion = value("--node-version", "22.19.0");

const nodeArchives = {
  "darwin-arm64": `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-darwin-arm64.tar.gz`,
  "darwin-x64": `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-darwin-x64.tar.gz`,
  "windows-x64": `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-win-x64.zip`,
  "windows-arm64": `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-win-arm64.zip`,
};

if (!nodeArchives[target]) throw new Error(`unsupported component target: ${target}`);

const run = (command, commandArgs, options = {}) =>
  execFileSync(command, commandArgs, { stdio: "inherit", ...options });

const hashFile = async (path) => {
  const bytes = await readFile(path);
  return createHash("sha256").update(bytes).digest("hex");
};

const work = mkdtempSync(join(tmpdir(), "codex-opencodex-component-"));
const stage = join(work, "component");
const nodeArchive = join(work, target.startsWith("windows-") ? "node.zip" : "node.tar.gz");
const nodeRoot = join(stage, "runtime");
const packageRoot = join(stage, "opencodex");

await mkdir(nodeRoot, { recursive: true });
await mkdir(packageRoot, { recursive: true });
await mkdir(resolve(output, ".."), { recursive: true });

run("curl", ["-fsSL", nodeArchives[target], "-o", nodeArchive]);
if (target.startsWith("windows-")) {
  const archive = nodeArchive.replaceAll("'", "''");
  const destination = work.replaceAll("'", "''");
  if (process.platform === "win32") {
    run("powershell", ["-NoProfile", "-NonInteractive", "-Command", `Expand-Archive -LiteralPath '${archive}' -DestinationPath '${destination}' -Force`]);
  } else {
    run("unzip", ["-q", nodeArchive, "-d", work]);
  }
  const extracted = join(work, `node-v${nodeVersion}-${target === "windows-arm64" ? "win-arm64" : "win-x64"}`);
  await cp(extracted, nodeRoot, { recursive: true });
} else {
  run("tar", ["-xzf", nodeArchive, "-C", nodeRoot, "--strip-components=1"]);
}

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const installArgs = ["install", "--prefix", packageRoot, "--omit=dev", `@bitkyc08/opencodex@${version}`];
const targetOs = target.startsWith("windows-") ? "win32" : "darwin";
const targetCpu = target.endsWith("arm64") ? "arm64" : "x64";
installArgs.push("--os=" + targetOs, "--cpu=" + targetCpu);
run(npm, installArgs, { env: { ...process.env, npm_config_fund: "false", npm_config_audit: "false" } });

const metadata = {
  schemaVersion: 1,
  component: "opencodex",
  version,
  target,
  nodeVersion,
  entry: target.startsWith("windows-") ? "runtime/node.exe" : "runtime/bin/node",
  launcher: "opencodex/node_modules/@bitkyc08/opencodex/bin/ocx.mjs",
  package: "@bitkyc08/opencodex",
  license: "MIT",
  source: "official npm package; Manager does not modify OpenCodex source",
};
await writeFile(join(stage, "component.json"), `${JSON.stringify(metadata, null, 2)}\n`);
await cp(join(packageRoot, "node_modules", "@bitkyc08", "opencodex", "LICENSE"), join(stage, "OPENCODEX-LICENSE"));

run("tar", ["-a", "-cf", output, "-C", stage, "."]);
const sha256 = await hashFile(output);
await writeFile(`${output}.sha256`, `${sha256}  ${output.split("/").at(-1)}\n`);
await rm(work, { recursive: true, force: true });
console.log(JSON.stringify({ output, sha256, metadata }, null, 2));
