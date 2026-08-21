const SKIN_DEEP_LINK_EVENT = "cam:skin-deep-link";

export function parseSkinDeepLink(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "osircodex:" || url.hostname !== "skin" || url.pathname !== "/install") return null;
    const id = url.searchParams.get("id") || "";
    return /^ver_[a-f0-9]{20}$/.test(id) ? id : null;
  } catch {
    return null;
  }
}

function dispatch(urls: string[]) {
  for (const url of urls) {
    const id = parseSkinDeepLink(url);
    if (id) window.dispatchEvent(new CustomEvent<string>(SKIN_DEEP_LINK_EVENT, { detail: id }));
  }
}

export async function initializeSkinDeepLinks(): Promise<() => void> {
  if (!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return () => undefined;
  const { getCurrent, onOpenUrl } = await import("@tauri-apps/plugin-deep-link");
  const current = await getCurrent().catch(() => null);
  if (current?.length) window.setTimeout(() => dispatch(current), 0);
  return onOpenUrl(dispatch);
}

export function onSkinDeepLink(handler: (id: string) => void) {
  const listener = (event: Event) => handler((event as CustomEvent<string>).detail);
  window.addEventListener(SKIN_DEEP_LINK_EVENT, listener);
  return () => window.removeEventListener(SKIN_DEEP_LINK_EVENT, listener);
}
