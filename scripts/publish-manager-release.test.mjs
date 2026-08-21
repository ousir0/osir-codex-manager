import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");

describe("unified manager release publisher", () => {
  it("uses the immutable GitHub release as the only artifact source", async () => {
    const script = await readFile(join(root, "scripts/publish-manager-release.sh"), "utf8");
    expect(script).toContain("gh release download");
    expect(script).toContain("sha256sum -c SHA256SUMS");
    expect(script).toContain("release must be published and immutable");
    expect(script).toContain("publish-rainyun.sh");
  });

  it("requires all four direct-download installer assets", async () => {
    const script = await readFile(join(root, "scripts/publish-manager-release.sh"), "utf8");
    for (const name of ["CodexManager_aarch64.dmg", "CodexManager_x86_64.dmg", "x64-setup.exe", "arm64-setup.exe"]) {
      expect(script).toContain(name);
    }
  });
});
