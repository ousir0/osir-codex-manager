import { describe, expect, it } from "vitest";

import { parseSkinDeepLink } from "./deepLinks";

describe("parseSkinDeepLink", () => {
  it("accepts only the pinned skin install route and DreamSkin version id", () => {
    expect(parseSkinDeepLink("osircodex://skin/install?id=ver_4c0255f97260110db5d2")).toBe(
      "ver_4c0255f97260110db5d2",
    );
  });

  it("rejects other schemes, hosts, paths, and malformed ids", () => {
    expect(parseSkinDeepLink("https://skin/install?id=ver_4c0255f97260110db5d2")).toBeNull();
    expect(parseSkinDeepLink("osircodex://other/install?id=ver_4c0255f97260110db5d2")).toBeNull();
    expect(parseSkinDeepLink("osircodex://skin/delete?id=ver_4c0255f97260110db5d2")).toBeNull();
    expect(parseSkinDeepLink("osircodex://skin/install?id=../../theme")).toBeNull();
  });
});
