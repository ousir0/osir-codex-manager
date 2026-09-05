import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { managerApi } from "../../services/managerApi";
import type { CodexConfigReport } from "../../shared/types";
import { I18nProvider } from "../i18n";
import { ThemeProvider } from "../theme";
import { CodexConfig } from "./CodexConfig";
import { OpenCodexPrototype } from "./OpenCodexPrototype";

const eventListeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: (event: { payload: unknown }) => void) => {
    eventListeners.set(name, callback);
    return () => eventListeners.delete(name);
  }),
}));

vi.mock("../../services/managerApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/managerApi")>();
  return {
    ...actual,
    managerApi: {
      ...actual.managerApi,
      codexConfigGet: vi.fn(),
      codexConfigActivateDefault: vi.fn(),
      codexConfigFetchModels: vi.fn(),
      codexConfigFetchImageModels: vi.fn(),
      codexConfigValidate: vi.fn(),
      codexConfigSaveRaw: vi.fn(),
      codexConfigSaveBasic: vi.fn(),
      codexConfigSetApiKey: vi.fn(),
      codexConfigDeleteApiKey: vi.fn(),
      codexConfigSetImageGenerationApiKey: vi.fn(),
      codexConfigDeleteImageGenerationApiKey: vi.fn(),
      codexConfigUpsertMcp: vi.fn(),
      codexConfigDeleteMcp: vi.fn(),
      codexConfigRestoreBackup: vi.fn(),
      openCodexStatus: vi.fn(),
      openCodexInstall: vi.fn(),
      openCodexStart: vi.fn(),
      openCodexActivateSaved: vi.fn(),
      openCodexConnectOsirOAuth: vi.fn(),
      openCodexCheckRoute: vi.fn(),
      openCodexHome: vi.fn(),
      macRestart: vi.fn(),
      winRestart: vi.fn(),
    },
  };
});

const api = vi.mocked(managerApi);

function config(overrides: Partial<CodexConfigReport> = {}): CodexConfigReport {
  const raw = `model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
base_url = "https://old.example/v1"

[mcp_servers.demo]
type = "stdio"
command = "npx"
args = ["-y", "demo"]
enabled = true

[mcp_servers.demo.env]
API_KEY = "secret-value"
`;
  return {
    path: "C:\\Users\\wei\\.codex\\config.toml",
    authPath: "C:\\Users\\wei\\.codex\\auth.json",
    exists: true, raw, redactedRaw: raw.replace("secret-value", "********"), parseError: null,
    model: "gpt-5", provider: "custom", baseUrl: "https://old.example/v1", reasoningEffort: "high", personality: "pragmatic", approvalPolicy: "never", sandboxMode: "danger-full-access", disableResponseStorage: true, goalMode: true,
    providers: [{ id: "custom", name: "Custom", baseUrl: "https://old.example/v1", wireApi: "responses" }],
    mcpServers: [{ name: "demo", enabled: true, transport: "stdio", command: "npx", args: ["-y", "demo"], url: null, hasSensitiveValues: true }],
    backupAvailable: true, apiKeyConfigured: true, authError: null, codexRunning: false, imageGenerationApiKeyConfigured: false,
    ...overrides,
  };
}

function renderConfig() {
  return render(<ThemeProvider><I18nProvider><CodexConfig onBack={vi.fn()} /></I18nProvider></ThemeProvider>);
}

describe("Codex configuration workbench", () => {
  beforeEach(() => {
    localStorage.setItem("cam.lang", "zh-CN");
    vi.clearAllMocks();
    eventListeners.clear();
    api.codexConfigGet.mockResolvedValue(config());
    api.codexConfigActivateDefault.mockResolvedValue(config());
    api.codexConfigFetchModels.mockResolvedValue(["gpt-5.6-sol", "gpt-5.6-terra"]);
    api.codexConfigFetchImageModels.mockResolvedValue(["gpt-image-2"]);
    api.codexConfigValidate.mockResolvedValue({ valid: true, error: null });
    api.codexConfigSaveBasic.mockImplementation(async (input) => config({ ...input }));
    api.codexConfigSaveRaw.mockImplementation(async (raw) => config({ raw, redactedRaw: raw }));
    api.codexConfigSetApiKey.mockResolvedValue(config({ apiKeyConfigured: true }));
    api.codexConfigDeleteApiKey.mockResolvedValue(config({ apiKeyConfigured: false }));
    api.codexConfigSetImageGenerationApiKey.mockResolvedValue(config({ imageGenerationApiKeyConfigured: true }));
    api.codexConfigDeleteImageGenerationApiKey.mockResolvedValue(config({ imageGenerationApiKeyConfigured: false }));
    api.codexConfigUpsertMcp.mockResolvedValue(config());
    api.codexConfigDeleteMcp.mockResolvedValue(config({ mcpServers: [] }));
    api.codexConfigRestoreBackup.mockResolvedValue(config());
    api.openCodexStatus.mockResolvedValue({ enabled: false, installed: false, version: null, port: 10100, serviceState: "missing", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 0, routes: [], backupAvailable: false, error: null, connectionStatus: "notConnected", account: null });
    api.openCodexInstall.mockResolvedValue({ enabled: false, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 0, routes: [], backupAvailable: true, error: null, connectionStatus: "notConnected", account: null });
    api.openCodexActivateSaved.mockResolvedValue({ enabled: true, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [], backupAvailable: true, error: null, connectionStatus: "connected", account: null });
    api.openCodexCheckRoute.mockResolvedValue({ routeId: "osirapi-openai", model: "gpt-5.6-sol", available: true, retryable: false, detail: "路由验证成功", checkedAt: String(Date.now()) });
    api.openCodexHome.mockResolvedValue();
    api.macRestart.mockResolvedValue();
    api.winRestart.mockResolvedValue();
  });

  it("keeps the restart prompt when the synced catalog is newer than running Codex", async () => {
    const user = userEvent.setup();
    api.openCodexStatus.mockResolvedValue({
      enabled: false,
      installed: true,
      version: "2.22.0",
      port: 10100,
      serviceState: "ready",
      codexProviderId: "opencodex",
      configPath: "~/.opencodex/config.json",
      catalogPath: "~/.codex/opencodex-catalog.json",
      modelCount: 31,
      routes: [],
      backupAvailable: true,
      error: null,
      connectionStatus: "connected",
      account: null,
      requiresCodexRestart: true,
    });
    renderConfig();
    const restart = await screen.findByRole("button", { name: "重启 Codex" });
    await user.click(restart);
    await waitFor(() => expect(api.macRestart).toHaveBeenCalledOnce());
  });

  it("shows a status-first overview and switches the selected default provider", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    expect(screen.getByText("当前调用路径：Codex → custom")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /OOSIR.*推荐/ }));
    await user.click(screen.getByRole("button", { name: "切换为当前供应商" }));
    await waitFor(() => expect(api.codexConfigSaveBasic).toHaveBeenCalledWith(expect.objectContaining({ provider: "osir", baseUrl: "https://api.osirclaw.com/v1" })));
  });

  it("keeps OpenCodex as a separate connection workspace", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: /OpenCodex 多模型/ }));
    expect(await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" })).toBeInTheDocument();
    expect(screen.getByText("尚未安装 OpenCodex")).toBeInTheDocument();
  });

  it("switches from an active OpenCodex workspace back to the default config file", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(config({ provider: "opencodex", baseUrl: "http://127.0.0.1:10100/v1", model: "osirapi-openai/gpt-5.6-sol", providers: [{ id: "osir", name: "OSIR", baseUrl: "https://api.osirclaw.com/v1", wireApi: "responses" }] }));
    api.openCodexStatus.mockResolvedValue({ enabled: true, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [], backupAvailable: true, error: null, connectionStatus: "connected", account: null });
    api.codexConfigActivateDefault.mockResolvedValue(config({ provider: "osir", baseUrl: "https://api.osirclaw.com/v1" }));
    renderConfig();
    await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" });
    await user.click(screen.getByRole("button", { name: /默认配置/ }));
    await user.click(screen.getByRole("button", { name: "启用默认配置" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "启用默认配置" }));
    await waitFor(() => expect(api.codexConfigActivateDefault).toHaveBeenCalledOnce());
    expect(await screen.findByRole("heading", { name: "默认配置" })).toBeInTheDocument();
  });

  it("keeps OpenCodex active when default gateway verification fails", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(config({ provider: "opencodex", baseUrl: "http://127.0.0.1:10100/v1" }));
    api.openCodexStatus.mockResolvedValue({ enabled: true, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [], backupAvailable: true, error: null, connectionStatus: "connected", account: null });
    api.codexConfigActivateDefault.mockRejectedValue(new Error("默认网关鉴权失败（status 401）"));
    renderConfig();
    await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" });
    await user.click(screen.getByRole("button", { name: /默认配置/ }));
    await user.click(screen.getByRole("button", { name: "启用默认配置" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "启用默认配置" }));
    expect(await screen.findByText(/默认网关鉴权失败/)).toBeInTheDocument();
    expect(api.codexConfigActivateDefault).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: /OpenCodex 多模型.*当前使用/ })).toBeInTheDocument();
  });

  it("activates saved OpenCodex routes from the mode switch", async () => {
    const user = userEvent.setup();
    api.openCodexStatus.mockResolvedValue({ enabled: false, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [{ id: "osirapi-openai", label: "GPT", adapter: "openai-responses", baseUrl: "https://api.osirclaw.com/v1", defaultModel: "gpt-5.6-sol", models: ["gpt-5.6-sol"], enabled: true, apiKeyConfigured: true, availability: "configured", locked: false }], backupAvailable: true, error: null, connectionStatus: "notConnected", account: null });
    api.codexConfigGet.mockResolvedValueOnce(config()).mockResolvedValueOnce(config({ provider: "opencodex", baseUrl: "http://127.0.0.1:10100/v1" }));
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: /OpenCodex 多模型/ }));
    await user.click(screen.getByRole("button", { name: "启用 OpenCodex 多模型" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "启用 OpenCodex 多模型" }));
    await waitFor(() => expect(api.openCodexActivateSaved).toHaveBeenCalledOnce());
    expect(await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" })).toBeInTheDocument();
  });

  it("prepares OpenCodex before opening a manual provider on a clean machine", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: /OpenCodex 多模型/ }));
    await user.click(screen.getByRole("button", { name: /手动添加供应商/ }));
    await waitFor(() => expect(api.openCodexInstall).toHaveBeenCalledOnce());
    expect(await screen.findByRole("heading", { name: "手动添加供应商" })).toBeInTheDocument();
  });

  it("shows enabled state after activating saved routes without an OSIR account", async () => {
    const stopped = await api.openCodexStatus();
    const saved = { ...stopped, installed: true, serviceState: "ready" as const, routes: [{ id: "custom", label: "Custom", adapter: "openai-responses", baseUrl: "https://example.test/v1", defaultModel: "gpt-6-astra", models: ["gpt-6-astra"], enabled: true, apiKeyConfigured: true, availability: "configured" as const, locked: false }] };
    api.openCodexStatus.mockResolvedValue(saved);
    api.openCodexActivateSaved.mockResolvedValue({ ...saved, enabled: true, connectionStatus: "connected" });
    const user = userEvent.setup();
    render(<OpenCodexPrototype />);
    await user.click(await screen.findByRole("button", { name: "启用已保存的多模型配置" }));
    expect(await screen.findByText("OpenCodex 多模型已启用", { exact: false, selector: '[role="status"]' })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /启用已保存|启动并启用/ })).not.toBeInTheDocument();
  });

  it("keeps activation failure visible when focus refresh only confirms a running service", async () => {
    const missing = await api.openCodexStatus();
    const saved = { ...missing, installed: true, serviceState: "ready" as const, routes: [{ id: "custom", label: "Custom", adapter: "openai-responses", baseUrl: "https://example.test/v1", defaultModel: "gpt-6-astra", models: ["gpt-6-astra"], enabled: true, apiKeyConfigured: true, availability: "configured" as const, locked: false }] };
    api.openCodexStatus.mockResolvedValue(saved);
    api.openCodexActivateSaved.mockRejectedValue(new Error("模型目录不完整，未启用多模型接管"));
    const user = userEvent.setup();
    render(<OpenCodexPrototype />);
    await user.click(await screen.findByRole("button", { name: "启用已保存的多模型配置" }));
    expect(await screen.findByText("模型目录不完整，未启用多模型接管")).toBeInTheDocument();
    const calls = api.openCodexStatus.mock.calls.length;
    const clock = vi.spyOn(Date, "now").mockReturnValue(Date.now() + 2000);
    fireEvent.focus(window);
    await waitFor(() => expect(api.openCodexStatus.mock.calls.length).toBeGreaterThan(calls));
    clock.mockRestore();
    expect(screen.getByText("模型目录不完整，未启用多模型接管")).toBeInTheDocument();
  });

  it("does not let a stale status read overwrite parent activation", async () => {
    const inactive = await api.openCodexStatus();
    let finishRead!: (value: typeof inactive) => void;
    api.openCodexStatus.mockImplementationOnce(() => new Promise(resolve => { finishRead = resolve; }));
    const view = render(<OpenCodexPrototype externalStatus={inactive} />);
    view.rerender(<OpenCodexPrototype externalStatus={{ ...inactive, installed: true, enabled: true, serviceState: "ready", connectionStatus: "signedOut" }} />);
    await act(async () => finishRead(inactive));
    expect(screen.getByText("OpenCodex 多模型已启用", { exact: false, selector: '[role="status"]' })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "安装多模型组件" })).not.toBeInTheDocument();
  });

  it("returns signed-out users to the OSIRAPI login flow", async () => {
    const signedOut = { ...(await api.openCodexStatus()), installed: true, enabled: false, serviceState: "ready" as const, connectionStatus: "signedOut" as const, routes: [{ id: "osirapi-openai", label: "GPT", adapter: "openai-responses", baseUrl: "https://api.osirclaw.com/v1", defaultModel: "gpt-5.6-sol", models: ["gpt-5.6-sol"], enabled: true, apiKeyConfigured: true, availability: "configured" as const, locked: false }] };
    api.openCodexStatus.mockResolvedValue(signedOut);
    const user = userEvent.setup();
    render(<OpenCodexPrototype />);

    await screen.findByText("已退出连接");
    await user.click(screen.getByRole("button", { name: "重新登录" }));
    expect(await screen.findByRole("heading", { name: "连接 OSIRAPI" })).toBeInTheDocument();
  });

  it("shows an honest empty route state instead of example models", async () => {
    api.openCodexStatus.mockResolvedValue({
      enabled: false, installed: true, version: "2.22.0", port: 10100, serviceState: "ready",
      codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json",
      modelCount: 0, routes: [], backupAvailable: true, error: null, connectionStatus: "notConnected", account: null,
    });
    render(<OpenCodexPrototype />);
    expect(await screen.findByText("尚未配置模型路由")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "0 个模型" })).toBeInTheDocument();
    expect(screen.queryByText("已准备 18 个模型")).not.toBeInTheDocument();
  });

  it("shows the detected platform and automatic install strategy", async () => {
    api.openCodexStatus.mockResolvedValue({ enabled: false, installed: false, version: null, port: 10100, serviceState: "missing", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 0, routes: [], backupAvailable: false, error: null, connectionStatus: "notConnected", account: null, environment: { platform: "windows", architecture: "x86_64", supported: true, runtimeState: "missing", installStrategy: "managedComponent", nodeVersion: null, npmAvailable: false, detail: "可自动准备私有运行时" } });
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: /OpenCodex 多模型/ }));
    expect(await screen.findByText("环境检测 · windows / x86_64")).toBeInTheDocument();
    expect(screen.getByText("将下载当前平台自带运行时")).toBeInTheDocument();
  });

  it("opens the multi-model workspace when the managed OpenCodex state is enabled", async () => {
    api.openCodexStatus.mockResolvedValue({ enabled: true, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [{ id: "osirapi-openai", label: "GPT", adapter: "openai-responses", baseUrl: "https://api.osirclaw.com/v1", defaultModel: "gpt-5.6-sol", models: ["gpt-5.6-sol"], enabled: true, apiKeyConfigured: true, availability: "configured", locked: false }], backupAvailable: true, error: null, connectionStatus: "error", account: null });
    renderConfig();

    expect(await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /OpenCodex 多模型.*当前使用/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("shows local setup progress after the browser authorization callback", async () => {
    api.openCodexStatus.mockResolvedValue({ enabled: false, installed: true, version: "2.22.0", port: 10100, serviceState: "ready", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 0, routes: [], backupAvailable: true, error: null, connectionStatus: "notConnected", account: null });
    api.openCodexConnectOsirOAuth.mockImplementation(() => new Promise(() => undefined));
    const user = userEvent.setup();
    renderConfig();
    await user.click(await screen.findByRole("button", { name: /OpenCodex 多模型/ }));
    await user.click(await screen.findByRole("button", { name: /^连接 OSIRAPI$/ }));
    await user.click(screen.getByRole("button", { name: "浏览器登录并连接" }));
    await waitFor(() => expect(eventListeners.has("opencodex://oauth-progress")).toBe(true));
    eventListeners.get("opencodex://oauth-progress")?.({ payload: { stage: "config", state: "running", step: 3, total: 4, title: "正在写入模型配置", detail: "保存订阅 Key、模型路由和 Codex 模型目录。" } });
    expect(await screen.findByText("正在写入模型配置")).toBeInTheDocument();
    expect(screen.getByText("读取账户与订阅").closest("div")).toHaveClass("complete");
    expect(screen.getByText("写入模型配置").closest("div")).toHaveClass("active");
  });

  it("keeps a completed OAuth connection and offers recheck after a transient 502", async () => {
    const user = userEvent.setup();
    const route = { id: "osirapi-openai", label: "GPT", adapter: "openai-responses", baseUrl: "https://api.osirclaw.com/v1", defaultModel: "gpt-5.6-sol", models: ["gpt-5.6-sol"], enabled: true, apiKeyConfigured: true, availability: "degraded" as const, locked: true };
    const warningStatus = { enabled: true, installed: true, version: "2.22.0", port: 10100, serviceState: "ready" as const, codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 1, routes: [route], backupAvailable: true, error: "授权和模型同步已完成，但默认路由遇到临时网络异常：502 Bad Gateway", connectionStatus: "connected" as const, account: { userId: 1, balance: 10, subscriptions: [] } };
    const verifiedStatus = { ...warningStatus, error: null, routes: [{ ...route, availability: "verified" as const }] };
    api.openCodexStatus.mockResolvedValueOnce(warningStatus).mockResolvedValue(verifiedStatus);
    api.openCodexConnectOsirOAuth.mockResolvedValue(warningStatus);

    renderConfig();
    await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" });
    await user.click(await screen.findByRole("button", { name: "管理连接" }));
    await user.click(await screen.findByRole("button", { name: "浏览器登录并连接" }));

    expect(await screen.findByRole("heading", { name: "OSIRAPI 已授权，等待模型复检" })).toBeInTheDocument();
    expect(screen.getByText("授权与模型同步已经完成，无需重新授权")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重新授权" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重新检测" }));
    await waitFor(() => expect(api.openCodexCheckRoute).toHaveBeenCalledWith("osirapi-openai", "gpt-5.6-sol"));
    expect(await screen.findByRole("heading", { name: "OSIRAPI 已授权并同步" })).toBeInTheDocument();
  });

  it("edits Codex behavior from a focused dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: "Codex 行为" }));
    await user.click(screen.getByRole("button", { name: "编辑行为设置" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "推理等级" }), "medium");
    await user.click(screen.getByRole("button", { name: "保存行为设置" }));
    await waitFor(() => expect(api.codexConfigSaveBasic).toHaveBeenCalledWith(expect.objectContaining({ reasoningEffort: "medium" })));
  });

  it("manages MCP from a list and opens the editor as a dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: /MCP 与工具/ }));
    const panel = screen.getByRole("region", { name: "MCP 与工具" });
    await user.click(within(panel).getAllByRole("button", { name: /demo/ })[0]);
    expect(screen.getByRole("heading", { name: "demo" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "取消" }));
    await user.click(within(panel).getByRole("switch", { name: "启用 demo" }));
    await waitFor(() => expect(api.codexConfigUpsertMcp).toHaveBeenCalledWith(expect.objectContaining({ originalName: "demo", enabled: false })));
  });

  it("keeps raw configuration masked until explicitly enabled", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: "高级与恢复" }));
    await user.click(screen.getByRole("button", { name: "编辑原始配置" }));
    const editor = screen.getByRole("textbox", { name: "config.toml" });
    expect(editor).toHaveAttribute("readonly");
    expect(editor).not.toHaveValue(expect.stringContaining("secret-value"));
    await user.click(screen.getByRole("switch", { name: "显示并编辑敏感值" }));
    expect(editor).not.toHaveAttribute("readonly");
    expect((editor as HTMLTextAreaElement).value).toContain("secret-value");
  });

  it("requires confirmation before deleting the API Key", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: "管理 API Key" }));
    await user.click(screen.getByRole("button", { name: "删除 API Key" }));
    expect(api.codexConfigDeleteApiKey).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "删除 API Key" });
    expect(within(dialog).getByText(/确认删除 Codex API Key/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "删除 API Key" }));
    await waitFor(() => expect(api.codexConfigDeleteApiKey).toHaveBeenCalledOnce());
  });

  it("shows an existing provider with its values in the default detail pane", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: /OOSIR.*推荐/ }));
    expect(screen.getByRole("heading", { name: "OSIR" })).toBeInTheDocument();
    expect(screen.getByText("osir", { selector: "small" })).toBeInTheDocument();
    expect(screen.getAllByText("https://api.osirclaw.com/v1").length).toBeGreaterThan(0);
  });

  it("explains the restart requirement when Codex is running", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(config({ codexRunning: true }));
    api.codexConfigSaveBasic.mockImplementation(async (input) => config({ ...input, codexRunning: true }));
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: "Codex 行为" }));
    await user.click(screen.getByRole("button", { name: "编辑行为设置" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "推理等级" }), "medium");
    await user.click(screen.getByRole("button", { name: "保存行为设置" }));
    await waitFor(() => expect(screen.getByText("配置已保存；重启 Codex 后生效")).toBeInTheDocument());
  });

  it("never fills a saved credential and only reveals a newly entered key", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("button", { name: "管理 API Key" }));
    const input = screen.getByLabelText("API Key");
    expect(input).toHaveValue("");
    expect(input).toHaveAttribute("type", "password");
    fireEvent.change(input, { target: { value: "sk-new-secret" } });
    await user.click(screen.getByRole("switch", { name: "显示正在输入的 API Key" }));
    expect(input).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));
    await waitFor(() => expect(api.codexConfigSetApiKey).toHaveBeenCalledWith("sk-new-secret"));
  });

  it("protects unsaved behavior edits before closing the dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: "Codex 行为" }));
    await user.click(screen.getByRole("button", { name: "编辑行为设置" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "推理等级" }), "medium");
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.getByRole("heading", { name: "放弃未保存的修改？" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "放弃修改" }));
    await user.click(screen.getByRole("button", { name: "编辑行为设置" }));
    expect(screen.getByRole("combobox", { name: "推理等级" })).toHaveValue("high");
  });

  it("opens image generation as an independent tool dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "默认配置" });
    await user.click(screen.getByRole("radio", { name: /MCP 与工具/ }));
    await user.click(screen.getByRole("button", { name: "配置图片生成" }));
    expect(screen.getByRole("heading", { name: "配置图片生成" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-image-2")).toBeInTheDocument();
  });
});
