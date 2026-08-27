import { describe, expect, it } from "vitest";
import { buildReleaseReport, collectGitFacts, generateReleaseReport } from "./release-report.mjs";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

describe("release report", () => {
  it("renders four platforms and redacts URL queries", async () => {
    const report = await buildReleaseReport({
      version: "0.5.29",
      git: { sha: "abc123", branch: "main", clean: true, status: [] },
      release: { draft: false, immutable: true, html_url: "https://github.com/ousir0/osir-codex-manager/releases/tag/v0.5.29?token=secret", assets: [{ name: "CodexManager_0.5.29_x64-setup.exe", size: 123, digest: "sha256:abc" }] },
      latest: { version: "0.5.29", platforms: { "windows-x86_64": { url: "https://app.osirclaw.com/manager/0.5.29/a.exe?sig=secret" } } },
      onlineChecks: { "windows-x86_64": { status: 200, size: 123 } },
    });
    expect(report).toContain("darwin-aarch64");
    expect(report).toContain("windows-aarch64");
    expect(report).toContain("HTTP");
    expect(report).toContain("Release immutable | 通过");
    expect(report).not.toContain("token=secret");
    expect(report).not.toContain("sig=secret");
  });

  it("collects git facts and preserves untracked-file visibility", async () => {
    const calls = [];
    const run = async (_cmd, args) => {
      calls.push(args);
      const last = args.at(-1);
      return { stdout: last === "HEAD" ? "sha\n" : last === "--show-current" ? "main\n" : "?? local.txt\n" };
    };
    const facts = await collectGitFacts("/tmp/repo", run);
    expect(facts).toMatchObject({ sha: "sha", branch: "main", clean: false, status: ["?? local.txt"] });
    expect(calls).toHaveLength(3);
  });

  it("supports offline JSON fixtures", async () => {
    const dir = await mkdtemp(join(tmpdir(), "release-report-"));
    const releasePath = join(dir, "release.json");
    const latestPath = join(dir, "latest.json");
    await writeFile(releasePath, JSON.stringify({ draft: false, isImmutable: true, assets: [] }));
    await writeFile(latestPath, JSON.stringify({ version: "0.5.29", platforms: {} }));
    const run = async (_cmd, args) => ({ stdout: args.at(-1) === "HEAD" ? "sha\n" : args.at(-1) === "--show-current" ? "main\n" : "" });
    const result = await generateReleaseReport({ version: "v0.5.29", output: join(dir, "report.md"), releaseJson: releasePath, latestJson: latestPath, cwd: "/tmp/repo", run, fetchImpl: async () => ({ status: 200, ok: true, url: "https://example.test", headers: { get: () => null }, json: async () => ({}) }) });
    expect(await readFile(result.target, "utf8")).toContain("Codex Manager v0.5.29");
  });

  it("attaches the report before immutable publication", async () => {
    const workflow = await readFile(join(import.meta.dirname, "..", ".github", "workflows", "release.yml"), "utf8");
    const draft = workflow.indexOf("- name: Upload GitHub Release draft");
    const report = workflow.indexOf("- name: Generate release acceptance report");
    const attach = workflow.indexOf("- name: Attach release acceptance report to draft");
    const publish = workflow.indexOf("- name: Publish GitHub Release");
    expect(draft).toBeGreaterThan(-1);
    expect(report).toBeGreaterThan(draft);
    expect(attach).toBeGreaterThan(report);
    expect(publish).toBeGreaterThan(attach);
    expect(workflow.slice(report, publish)).toContain("release-report.mjs");
    expect(workflow.slice(attach, publish)).toContain("gh release upload");
  });
});
