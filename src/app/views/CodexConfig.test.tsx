import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { managerApi } from "../../services/managerApi";
import type { CodexConfigReport } from "../../shared/types";
import { I18nProvider } from "../i18n";
import { ThemeProvider } from "../theme";
import { CodexConfig } from "./CodexConfig";

vi.mock("../../services/managerApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/managerApi")>();
  return {
    ...actual,
    managerApi: {
      ...actual.managerApi,
      codexConfigGet: vi.fn(),
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
      openCodexConnectOsirOAuth: vi.fn(),
      openCodexHome: vi.fn(),
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
    api.codexConfigGet.mockResolvedValue(config());
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
    api.openCodexInstall.mockResolvedValue({ enabled: false, installed: true, version: "2.22.0", port: 10100, serviceState: "stopped", codexProviderId: "opencodex", configPath: "~/.opencodex/config.json", catalogPath: "~/.codex/opencodex-catalog.json", modelCount: 0, routes: [], backupAvailable: true, error: null, connectionStatus: "notConnected", account: null });
    api.openCodexHome.mockResolvedValue();
  });

  it("shows a status-first overview and edits a provider in a dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
    expect(screen.getByText("当前调用路径：Codex → custom")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /OOSIR.*推荐/ }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "配置连接" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("textbox", { name: "模型" })).toHaveValue("gpt-5");
    await user.click(screen.getByRole("button", { name: "保存并启用" }));
    await waitFor(() => expect(api.codexConfigSaveBasic).toHaveBeenCalledWith(expect.objectContaining({ provider: "osir", baseUrl: "https://api.osirclaw.com/v1" })));
  });

  it("keeps OpenCodex as a separate connection workspace", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
    await user.click(screen.getByRole("button", { name: /OpenCodex 多模型/ }));
    expect(await screen.findByRole("heading", { name: "把所有模型，装进 Codex 选择器。" })).toBeInTheDocument();
    expect(screen.getByText("尚未安装 OpenCodex")).toBeInTheDocument();
  });

  it("edits Codex behavior from a focused dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
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
    await screen.findByRole("heading", { name: "单供应商直连" });
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
    await screen.findByRole("heading", { name: "单供应商直连" });
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
    await screen.findByRole("heading", { name: "单供应商直连" });
    await user.click(screen.getByRole("button", { name: "管理 API Key" }));
    await user.click(screen.getByRole("button", { name: "删除 API Key" }));
    expect(api.codexConfigDeleteApiKey).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "删除 API Key" });
    expect(within(dialog).getByText(/确认删除 Codex API Key/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "删除 API Key" }));
    await waitFor(() => expect(api.codexConfigDeleteApiKey).toHaveBeenCalledOnce());
  });

  it("opens an existing provider with its values in the connection dialog", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
    await user.click(screen.getByRole("button", { name: /OOSIR.*推荐/ }));
    expect(screen.getByRole("heading", { name: "配置连接" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("osir")).toBeInTheDocument();
    expect(screen.getByDisplayValue("https://api.osirclaw.com/v1")).toBeInTheDocument();
  });

  it("explains the restart requirement when Codex is running", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(config({ codexRunning: true }));
    api.codexConfigSaveBasic.mockImplementation(async (input) => config({ ...input, codexRunning: true }));
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
    await user.click(screen.getByRole("radio", { name: "Codex 行为" }));
    await user.click(screen.getByRole("button", { name: "编辑行为设置" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "推理等级" }), "medium");
    await user.click(screen.getByRole("button", { name: "保存行为设置" }));
    await waitFor(() => expect(screen.getByText("配置已保存；重启 Codex 后生效")).toBeInTheDocument());
  });

  it("never fills a saved credential and only reveals a newly entered key", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByRole("heading", { name: "单供应商直连" });
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
    await screen.findByRole("heading", { name: "单供应商直连" });
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
    await screen.findByRole("heading", { name: "单供应商直连" });
    await user.click(screen.getByRole("radio", { name: /MCP 与工具/ }));
    await user.click(screen.getByRole("button", { name: "配置图片生成" }));
    expect(screen.getByRole("heading", { name: "配置图片生成" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-image-2")).toBeInTheDocument();
  });
});
