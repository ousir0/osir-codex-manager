function toStaticError(value: unknown): Error {
  if (value instanceof Error) return value;
  if (value && typeof value === "object" && "message" in value) {
    const message = (value as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return new Error(message);
  }
  return new Error(String(value ?? "Unknown bootstrap error"));
}

function errorSummary(value: unknown): string {
  const error = toStaticError(value);
  return `${error.name}: ${error.message}`;
}

function recoveryRoot(doc: Document): HTMLElement {
  const existing = doc.getElementById("root");
  if (existing instanceof HTMLElement) return existing;
  const root = doc.createElement("div");
  root.id = "root";
  (doc.body ?? doc.documentElement).appendChild(root);
  return root;
}

function staticErrorMessage(value: unknown): string {
  return toStaticError(value).message || "The backend kept the app open.";
}

type StaticTauriInternals = {
  invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
};

function invokeStaticBackend<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const internals = (
    window as typeof window & { __TAURI_INTERNALS__?: StaticTauriInternals }
  ).__TAURI_INTERNALS__;
  if (typeof internals?.invoke !== "function") {
    return Promise.reject(new Error("Desktop backend unavailable."));
  }
  try {
    return Promise.resolve(internals.invoke<T>(command, args));
  } catch (cause) {
    return Promise.reject(cause);
  }
}

function reportStaticCrash(value: unknown): void {
  const error = toStaticError(value);
  try {
    console.error("[bootstrap]", error);
  } catch {
    // Logging itself must not become another blank-window path.
  }
  void invokeStaticBackend<void>("log_frontend_error", {
    payload: {
      kind: "bootstrap",
      message: error.message,
      stack: error.stack ?? null,
      componentStack: null,
    },
  }).catch(() => undefined);
}

function phaseAwareQuit(): Promise<void> {
  // Call the custom backend command directly: this page exists precisely when
  // normal React/service imports may be broken. The backend still rejects
  // committing/finishing phases, so the recovery control cannot corrupt work.
  return invokeStaticBackend<void>("confirm_quit");
}

function matchesPhysicalKey(event: KeyboardEvent, value: string): boolean {
  const key = event.key.toLowerCase();
  const code = event.code.toLowerCase();
  if (value.length === 1 && /[a-z]/.test(value)) {
    return key === value || code === `key${value}`;
  }
  return key === value || code === value;
}

/** Browser-only commands that must never escape the static release surface. */
export function shouldBlockStaticCrashShortcut(event: KeyboardEvent): boolean {
  const key = event.key.toLowerCase();
  const code = event.code.toLowerCase();
  if (
    ["f3", "f5", "f7", "f12", "browserback", "browserforward", "browserrefresh", "browsersearch"].includes(
      key,
    ) ||
    ["f3", "f5", "f7", "f12", "browserback", "browserforward", "browserrefresh", "browsersearch"].includes(
      code,
    )
  ) {
    return true;
  }

  const primary = event.ctrlKey || event.metaKey;
  if (
    primary &&
    ["f", "g", "j", "l", "o", "p", "r", "s", "u"].some((value) =>
      matchesPhysicalKey(event, value),
    )
  ) {
    return true;
  }
  if (
    primary &&
    (["0", "+", "-", "="].includes(key) ||
      ["digit0", "equal", "minus", "numpadadd", "numpadsubtract", "numpad0"].includes(code))
  ) {
    return true;
  }
  if (
    (event.ctrlKey && event.shiftKey && ["i", "j", "c"].some((value) => matchesPhysicalKey(event, value))) ||
    (event.metaKey && event.altKey && ["i", "j", "c"].some((value) => matchesPhysicalKey(event, value)))
  ) {
    return true;
  }
  if (
    event.metaKey &&
    (key === "[" || key === "]" || code === "bracketleft" || code === "bracketright")
  ) {
    return true;
  }
  return event.altKey && (key === "arrowleft" || key === "arrowright");
}

const staticPolicyDisposers = new WeakMap<Document, () => void>();

/** Install the full release browser-chrome policy at the document boundary. */
export function installStaticCrashPolicy(
  doc: Document = document,
  enabled = !import.meta.env.DEV,
): () => void {
  if (!enabled) return () => {};
  const installed = staticPolicyDisposers.get(doc);
  if (installed) return installed;

  const stop = (event: Event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (shouldBlockStaticCrashShortcut(event)) stop(event);
  };
  const onMouseNavigation = (event: MouseEvent) => {
    if (event.button === 3 || event.button === 4) stop(event);
  };

  doc.addEventListener("contextmenu", stop, true);
  doc.addEventListener("keydown", onKeyDown, true);
  doc.addEventListener("mousedown", onMouseNavigation, true);
  doc.addEventListener("mouseup", onMouseNavigation, true);
  doc.addEventListener("auxclick", onMouseNavigation, true);

  const dispose = () => {
    doc.removeEventListener("contextmenu", stop, true);
    doc.removeEventListener("keydown", onKeyDown, true);
    doc.removeEventListener("mousedown", onMouseNavigation, true);
    doc.removeEventListener("mouseup", onMouseNavigation, true);
    doc.removeEventListener("auxclick", onMouseNavigation, true);
    if (staticPolicyDisposers.get(doc) === dispose) staticPolicyDisposers.delete(doc);
  };
  staticPolicyDisposers.set(doc, dispose);
  return dispose;
}

/** Remove a previously installed policy (used by tests and hot reload). */
export function disposeStaticCrashPolicy(doc: Document = document): void {
  staticPolicyDisposers.get(doc)?.();
}

/** Plain-DOM fallback for failures before React can load, create or render its root. */
export function renderStaticCrashFallback(
  value: unknown,
  doc: Document = document,
  productionPolicy = !import.meta.env.DEV,
): HTMLElement {
  reportStaticCrash(value);
  installStaticCrashPolicy(doc, productionPolicy);
  const root = recoveryRoot(doc);
  const main = doc.createElement("main");
  main.dataset.staticCrash = "true";
  main.setAttribute("role", "alert");
  // Do not depend on the normal Shell, theme tokens or even the stylesheet for
  // legibility: the Tauri window and body are transparent by design.
  Object.assign(main.style, {
    minHeight: "100vh",
    padding: "24px",
    borderRadius: "16px",
    background: "#17171d",
    color: "#f7f6fa",
    fontFamily: "system-ui, sans-serif",
    display: "flex",
    flexDirection: "column",
    gap: "14px",
    overflow: "auto",
  });

  const title = doc.createElement("h1");
  title.textContent = "Codex Manager could not start / 管理器界面无法启动";
  const body = doc.createElement("p");
  body.textContent =
    "Reload the interface. Any backend operation remains protected. / 请重新加载界面；后台操作仍受保护。";
  const detail = doc.createElement("pre");
  detail.textContent = errorSummary(value);
  const reload = doc.createElement("button");
  reload.type = "button";
  reload.textContent = "Reload / 重新加载";
  reload.addEventListener("click", () => window.location.reload());
  const quit = doc.createElement("button");
  quit.type = "button";
  quit.textContent = "Quit safely / 安全退出";
  const quitFailure = doc.createElement("p");
  quitFailure.setAttribute("role", "status");
  quit.addEventListener("click", () => {
    quit.disabled = true;
    quitFailure.textContent = "";
    void phaseAwareQuit()
      .catch((cause) => {
        quitFailure.setAttribute("role", "alert");
        quitFailure.textContent = staticErrorMessage(cause);
      })
      .finally(() => {
        quit.disabled = false;
      });
  });

  main.append(title, body, detail, reload, quit, quitFailure);
  root.replaceChildren(main);
  return main;
}
