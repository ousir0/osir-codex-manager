import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { managerApi, type ManagerUpdateAvailable } from "../services/managerApi";
import { ThemeProvider } from "./theme";
import { I18nProvider } from "./i18n";
import { ManagerUpdateProvider, useManagerUpdate } from "./ManagerUpdateProvider";

vi.mock("../services/managerApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../services/managerApi")>();
  return {
    ...actual,
    managerApi: {
      ...actual.managerApi,
      getSettings: vi.fn(),
      checkManagerUpdate: vi.fn(),
    },
  };
});

const api = vi.mocked(managerApi);

function Harness() {
  const { check } = useManagerUpdate();
  return <button type="button" onClick={() => void check()}>检查管理器更新</button>;
}

function update(overrides: Partial<ManagerUpdateAvailable> = {}): ManagerUpdateAvailable {
  return {
    kind: "available",
    version: "0.5.6",
    currentVersion: "0.5.5",
    body: "修复更新闭环",
    installAndRelaunch: vi.fn().mockResolvedValue(undefined),
    discard: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

function renderProvider() {
  return render(
    <ThemeProvider>
      <I18nProvider>
        <ManagerUpdateProvider>
          <Harness />
        </ManagerUpdateProvider>
      </I18nProvider>
    </ThemeProvider>,
  );
}

describe("ManagerUpdateProvider", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    vi.clearAllMocks();
    api.getSettings.mockResolvedValue({
      source: "auto", customUrl: "", autoCheck: false, checkOnStartup: false,
      periodicCheck: false, periodicCheckIntervalSeconds: 900, askBefore: true,
      signedOnly: true, confirmClose: true, windowsInstallMode: "msix",
      installRoot: "%LOCALAPPDATA%\\Programs\\Codex", proxyMode: "system",
      customProxyUrl: "", disableCodexSelfUpdates: false, skippedCodexUpdate: null,
      codexTheme: null, codexThemeDir: null, codexThemeStoreDir: null, skinGroups: [],
    });
    api.checkManagerUpdate.mockResolvedValue({ kind: "none" });
  });

  it("shows a global update dialog after a background check finds a new version", async () => {
    const next = update();
    api.checkManagerUpdate.mockResolvedValue(next);
    const user = userEvent.setup();
    renderProvider();

    await user.click(screen.getByRole("button", { name: "检查管理器更新" }));
    expect(await screen.findByRole("dialog")).toHaveTextContent("0.5.6");
    expect(screen.getByText("修复更新闭环")).toBeInTheDocument();
  });

  it("skips only the offered version", async () => {
    api.checkManagerUpdate.mockResolvedValue(update());
    const user = userEvent.setup();
    renderProvider();

    await user.click(screen.getByRole("button", { name: "检查管理器更新" }));
    await user.click(await screen.findByRole("button", { name: /Skip current|跳过当前/ }));
    expect(localStorage.getItem("cam.manager.update.skipped")).toBe("0.5.6");

    api.checkManagerUpdate.mockResolvedValue(update({ version: "0.5.7" }));
    await user.click(screen.getByRole("button", { name: "检查管理器更新" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toHaveTextContent("0.5.7"));
  });

  it("starts the signed updater install after confirmation", async () => {
    const installAndRelaunch = vi.fn().mockResolvedValue(undefined);
    api.checkManagerUpdate.mockResolvedValue(update({ installAndRelaunch }));
    const user = userEvent.setup();
    renderProvider();

    await user.click(screen.getByRole("button", { name: "检查管理器更新" }));
    await user.click(await screen.findByRole("button", { name: /Update|确定/ }));
    await waitFor(() => expect(installAndRelaunch).toHaveBeenCalledOnce());
    expect(localStorage.getItem("cam.manager.update.pending")).toContain("0.5.6");
  });

  it("clears a completed pending update after the new version starts", async () => {
    localStorage.setItem("cam.manager.update.pending", JSON.stringify({ version: "0.5.5", startedAt: Date.now() }));
    api.checkManagerUpdate.mockResolvedValue({ kind: "none" });
    renderProvider();

    await waitFor(() => expect(api.checkManagerUpdate).toHaveBeenCalled());
    expect(localStorage.getItem("cam.manager.update.pending")).toBeNull();
  });
});
