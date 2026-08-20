import { render, screen, waitFor, within } from "@testing-library/react";
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
      codexConfigGet: vi.fn(),
      codexConfigFetchModels: vi.fn(),
      codexConfigValidate: vi.fn(),
      codexConfigSaveRaw: vi.fn(),
      codexConfigSaveBasic: vi.fn(),
      codexConfigSetApiKey: vi.fn(),
      codexConfigDeleteApiKey: vi.fn(),
      codexConfigUpsertMcp: vi.fn(),
      codexConfigDeleteMcp: vi.fn(),
      codexConfigRestoreBackup: vi.fn(),
      openCodexStatus: vi.fn(),
      openCodexInstall: vi.fn(),
      openCodexStart: vi.fn(),
      openCodexConnectOsir: vi.fn(),
      openCodexSave: vi.fn(),
      openCodexSync: vi.fn(),
      openCodexRestore: vi.fn(),
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
    exists: true,
    raw,
    redactedRaw: raw.replace("secret-value", "********"),
    parseError: null,
    model: "gpt-5",
    provider: "custom",
    baseUrl: "https://old.example/v1",
    reasoningEffort: "high",
    personality: "pragmatic",
    approvalPolicy: "never",
    sandboxMode: "danger-full-access",
    disableResponseStorage: true,
    goalMode: true,
    providers: [
      {
        id: "custom",
        name: "Custom",
        baseUrl: "https://old.example/v1",
        wireApi: "responses",
      },
    ],
    mcpServers: [
      {
        name: "demo",
        enabled: true,
        transport: "stdio",
        command: "npx",
        args: ["-y", "demo"],
        url: null,
        hasSensitiveValues: true,
      },
    ],
    backupAvailable: true,
    apiKeyConfigured: true,
    authError: null,
    codexRunning: false,
    ...overrides,
  };
}

function renderConfig() {
  return render(
    <ThemeProvider>
      <I18nProvider>
        <CodexConfig onBack={vi.fn()} />
      </I18nProvider>
    </ThemeProvider>,
  );
}

describe("Codex configuration manager", () => {
  beforeEach(() => {
    localStorage.setItem("cam.lang", "zh-CN");
    vi.clearAllMocks();
    api.codexConfigGet.mockResolvedValue(config());
    api.codexConfigFetchModels.mockResolvedValue(["gpt-5.6-sol", "gpt-5.6-terra"]);
    api.codexConfigValidate.mockResolvedValue({ valid: true, error: null });
    api.codexConfigSaveRaw.mockImplementation(async (raw) => config({ raw, redactedRaw: raw }));
    api.codexConfigSaveBasic.mockImplementation(async (input) =>
      config({
        model: input.model,
        provider: input.provider,
        baseUrl: input.baseUrl,
        reasoningEffort: input.reasoningEffort,
        personality: input.personality,
        approvalPolicy: input.approvalPolicy,
        sandboxMode: input.sandboxMode,
        disableResponseStorage: input.disableResponseStorage,
        goalMode: input.goalMode,
      }),
    );
    api.codexConfigSetApiKey.mockResolvedValue(config({ apiKeyConfigured: true }));
    api.codexConfigDeleteApiKey.mockResolvedValue(config({ apiKeyConfigured: false }));
    api.codexConfigUpsertMcp.mockResolvedValue(config());
    api.codexConfigDeleteMcp.mockResolvedValue(config({ mcpServers: [] }));
    api.codexConfigRestoreBackup.mockResolvedValue(config());
    api.openCodexStatus.mockResolvedValue({
      enabled: false,
      installed: false,
      version: null,
      port: 10100,
      serviceState: "missing",
      codexProviderId: "opencodex",
      configPath: "~/.opencodex/config.json",
      catalogPath: "~/.codex/opencodex-catalog.json",
      modelCount: 0,
      routes: [],
      backupAvailable: false,
      error: null,
      connectionStatus: "notConnected",
      account: null,
    });
    api.openCodexInstall.mockResolvedValue({
      enabled: false,
      installed: true,
      version: "2.22.0",
      port: 10100,
      serviceState: "stopped",
      codexProviderId: "opencodex",
      configPath: "~/.opencodex/config.json",
      catalogPath: "~/.codex/opencodex-catalog.json",
      modelCount: 0,
      routes: [],
      backupAvailable: true,
      error: null,
      connectionStatus: "notConnected",
      account: null,
    });
    api.openCodexHome.mockResolvedValue();
  });

  it("fills the OSIR provider preset and saves it as structured config", async () => {
    const user = userEvent.setup();
    renderConfig();

    await screen.findByDisplayValue("gpt-5");
    await user.click(screen.getByRole("button", { name: /OSIR.*推荐/ }));

    expect(screen.getByDisplayValue("osir")).toBeInTheDocument();
    expect(screen.getByDisplayValue("https://api.osirclaw.com/v1")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /保存基础配置/ }));

    await waitFor(() =>
      expect(api.codexConfigSaveBasic).toHaveBeenCalledWith({
        model: "gpt-5",
        provider: "osir",
        baseUrl: "https://api.osirclaw.com/v1",
        reasoningEffort: "high",
        personality: "pragmatic",
        approvalPolicy: "never",
        sandboxMode: "danger-full-access",
        disableResponseStorage: true,
        goalMode: true,
        imageGenerationCompatibility: false,
      }),
    );
  });

  it("lists multiple providers and switches the active draft without deleting the others", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(
      config({
        providers: [
          {
            id: "custom",
            name: "Custom",
            baseUrl: "https://old.example/v1",
            wireApi: "responses",
          },
          {
            id: "backup",
            name: "Backup",
            baseUrl: "https://backup.example/v1",
            wireApi: "chat",
          },
        ],
      }),
    );
    renderConfig();

    await screen.findByDisplayValue("gpt-5");
    expect(screen.getByText("已配置供应商")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Backup.*backup.*https:\/\/backup\.example/ }));
    expect(screen.getByDisplayValue("backup")).toBeInTheDocument();
    expect(screen.getByDisplayValue("https://backup.example/v1")).toBeInTheDocument();
  });

  it("opens the multi-model workbench and starts the component install flow", async () => {
    const user = userEvent.setup();
    renderConfig();

    await screen.findByDisplayValue("gpt-5");
    await user.click(screen.getByText("多模型", { exact: true }));
    expect(screen.getByRole("heading", { name: "把所有模型，装进 Codex 选择器。" })).toBeInTheDocument();
    expect(screen.getByText("尚未安装 OpenCodex")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "安装多模型组件" }));
    await waitFor(() => expect(api.openCodexInstall).toHaveBeenCalledOnce());
  });

  it("allows saving while Codex is running and explains that restart is required", async () => {
    const user = userEvent.setup();
    api.codexConfigGet.mockResolvedValue(config({ codexRunning: true }));
    api.codexConfigSaveBasic.mockImplementation(async (input) =>
      config({ ...input, codexRunning: true }),
    );
    renderConfig();

    const model = await screen.findByDisplayValue("gpt-5");
    expect(screen.getByRole("status")).toHaveTextContent(/修改可以保存/);
    expect(model).not.toBeDisabled();
    await user.click(screen.getByRole("button", { name: /保存基础配置/ }));

    await waitFor(() => expect(api.codexConfigSaveBasic).toHaveBeenCalledOnce());
    expect(screen.getByText("配置已保存；重启 Codex 后生效")).toBeInTheDocument();
  });

  it("keeps OSIR as the single recommended provider and places providers beside details", async () => {
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    expect(screen.getByRole("button", { name: /OSIR.*推荐/ })).toBeInTheDocument();
    expect(screen.getByText("推荐供应商")).toBeInTheDocument();
    expect(screen.getByLabelText("已配置供应商")).toBeInTheDocument();
    expect(screen.getByLabelText("当前供应商详情")).toBeInTheDocument();
  });

  it("fetches models from the selected provider and keeps the model editable", async () => {
    const user = userEvent.setup();
    renderConfig();

    const model = await screen.findByLabelText("模型");
    expect(model.tagName).toBe("INPUT");
    await user.click(screen.getByRole("button", { name: "获取模型" }));
    await waitFor(() =>
      expect(api.codexConfigFetchModels).toHaveBeenCalledWith("https://old.example/v1"),
    );
    expect(screen.getByText("已获取 2 个模型")).toBeInTheDocument();

    const modelSelect = screen.getByRole("combobox", { name: "模型" });
    expect(modelSelect).toHaveValue("gpt-5");
    await user.selectOptions(modelSelect, "gpt-5.6-sol");
    expect(modelSelect).toHaveValue("gpt-5.6-sol");
  });

  it("warns before saving the unrestricted no-approval combination", async () => {
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    expect(screen.getByRole("alert")).toHaveTextContent(/无需确认访问系统全部文件/);
  });

  it("masks raw secrets until the user explicitly reveals them", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    await user.click(screen.getByRole("radio", { name: "高级" }));
    const editor = screen.getByRole("textbox", { name: "config.toml" });
    expect(editor).toHaveAttribute("readonly");
    expect((editor as HTMLTextAreaElement).value).toContain("********");
    expect((editor as HTMLTextAreaElement).value).not.toContain("secret-value");

    await user.click(screen.getByRole("switch", { name: "显示敏感值" }));
    expect(editor).not.toHaveAttribute("readonly");
    expect((editor as HTMLTextAreaElement).value).toContain("secret-value");
  });

  it("never refills a saved API Key and masks newly entered credentials", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    const input = screen.getByLabelText("API Key");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveValue("");
    expect(screen.getByText("已配置")).toBeInTheDocument();

    await user.type(input, "sk-new-secret");
    await user.click(screen.getByRole("switch", { name: "显示正在输入的 API Key" }));
    expect(input).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));

    await waitFor(() =>
      expect(api.codexConfigSetApiKey).toHaveBeenCalledWith("sk-new-secret"),
    );
    expect(input).toHaveValue("");
    expect(input).toHaveAttribute("type", "password");
  });

  it("requires confirmation before deleting the API Key", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    await user.click(screen.getByRole("button", { name: "删除 API Key" }));
    expect(api.codexConfigDeleteApiKey).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText(/从 auth\.json 删除/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "删除 API Key" }));

    await waitFor(() => expect(api.codexConfigDeleteApiKey).toHaveBeenCalledOnce());
  });

  it("sends a narrow MCP toggle update with the existing command fields", async () => {
    const user = userEvent.setup();
    renderConfig();
    await screen.findByDisplayValue("gpt-5");

    await user.click(screen.getByRole("radio", { name: "MCP (1)" }));
    const panel = screen.getByRole("region", { name: "MCP" });
    await user.click(within(panel).getByRole("switch"));

    await waitFor(() =>
      expect(api.codexConfigUpsertMcp).toHaveBeenCalledWith({
        originalName: "demo",
        name: "demo",
        enabled: false,
        transport: "stdio",
        command: "npx",
        args: ["-y", "demo"],
        url: null,
      }),
    );
  });
});
