// Which desktop platform are we rendering for? The backend commands are
// platform-specific (mac_* vs win_*), so the UI dispatches on this.

export type Platform = "macos" | "windows" | "other";

export function currentPlatform(): Platform {
  // Keep browser previews deterministic without changing production platform
  // detection. Example: `/?platform=windows` in a Vite dev build.
  if (import.meta.env.DEV && typeof window !== "undefined") {
    const override = new URLSearchParams(window.location.search).get("platform");
    if (override === "macos" || override === "windows" || override === "other") {
      return override;
    }
  }
  const p = (navigator.platform || "").toLowerCase();
  const ua = (navigator.userAgent || "").toLowerCase();
  if (p.startsWith("mac") || ua.includes("mac os") || ua.includes("macintosh")) {
    return "macos";
  }
  if (p.startsWith("win") || ua.includes("windows")) {
    return "windows";
  }
  return "other";
}

export function isWindows(): boolean {
  return currentPlatform() === "windows";
}
