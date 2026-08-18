import { useCallback, useEffect, useMemo, useState } from "react";

import { errorMessage, managerApi } from "../../services/managerApi";
import type {
  CodexBasicConfigInput,
  CodexConfigReport,
  CodexMcpServer,
  CodexMcpServerInput,
  CodexMcpTransport,
} from "../../shared/types";
import { NavBar, Segmented, Toggle } from "../components";
import { Icon } from "../icons";
import { useI18n } from "../i18n";
import { OpenCodexPrototype } from "./OpenCodexPrototype";

type ConfigTab = "basic" | "multi" | "mcp" | "advanced";

interface McpDraft extends CodexMcpServerInput {
  argsText: string;
}

const EMPTY_BASIC: CodexBasicConfigInput = {
  model: "gpt-5.6-sol",
  provider: "",
  baseUrl: "",
  reasoningEffort: "",
  personality: "",
  approvalPolicy: "",
  sandboxMode: "",
  disableResponseStorage: false,
  goalMode: false,
  imageGenerationCompatibility: false,
};

const ZH_COPY = {
  loading: "正在读取 Codex 配置…",
  file: "配置文件",
  openFolder: "打开目录",
  fileMissing: "保存后将创建 config.toml",
  backupReady: "可恢复上一版本",
  invalid: "当前 TOML 有错误，请在高级编辑中修复",
  running: "Codex 正在运行，修改可以保存；重启 Codex 后生效",
  basic: "基础",
  multi: "多模型",
  mcp: "MCP",
  advanced: "高级",
  model: "模型",
  modelPlaceholder: "例如 gpt-5.6-sol",
  fetchModels: "获取模型",
  modelsFetched: "已获取 {count} 个模型",
  provider: "供应商标识",
  providerPlaceholder: "例如 osir",
  providers: "已配置供应商",
  providerCount: "{count} 个",
  newProvider: "新建供应商",
  providerSelected: "当前",
  providerDetails: "当前供应商详情",
  recommended: "推荐",
  recommendedProvider: "推荐供应商",
  baseUrl: "Base URL",
  credentials: "API 凭据",
  authFile: "凭据文件",
  apiKey: "API Key",
  imageApiKey: "生图 API Key",
  imageApiKeyPlaceholder: "输入独立图片 API Key",
  imageApiKeyHint: "保存到 ~/.codex/imagegen-relay.json，并自动安装 imagegen-relay 技能；不会写入 config.toml。",
  imageModel: "生图模型",
  imageBaseUrl: "生图 API Base URL",
  fetchImageModels: "获取生图模型",
  imageModelPlaceholder: "默认 gpt-image-2；可点击获取模型后选择",
  saveImageApiKey: "保存并安装技能",
  deleteImageApiKey: "删除生图 API Key",
  apiKeyPlaceholder: "输入新的 API Key",
  apiKeyConfigured: "已配置",
  apiKeyMissing: "未配置",
  apiKeyHint: "已保存的密钥不会回显",
  showApiKey: "显示正在输入的 API Key",
  saveApiKey: "保存 API Key",
  apiKeySaved: "API Key 已安全保存",
  deleteApiKey: "删除 API Key",
  deleteApiKeyConfirm: "确认从 auth.json 删除 API Key？",
  apiKeyDeleted: "API Key 已删除",
  reasoning: "推理等级",
  personality: "Personality",
  goalMode: "Goal Mode",
  disableResponseStorage: "禁用响应存储",
  imageGenerationCompatibility: "第三方中转生图兼容模式",
  imageGenerationCompatibilityHint: "开启后使用独立图片 API 技能；聊天继续使用普通 API Key。修改后重启 Codex。",
  executionAccess: "执行权限",
  approvalPolicy: "审批策略",
  sandboxMode: "沙箱模式",
  dangerousCombination: "当前组合允许 Codex 无需确认访问系统全部文件并执行命令。仅在你完全信任当前任务时使用。",
  automatic: "跟随 Codex 默认",
  saveBasic: "保存基础配置",
  saved: "配置已保存，并保留了上一版本备份",
  savedRunning: "配置已保存；重启 Codex 后生效",
  emptyMcp: "尚未配置 MCP 服务器",
  addMcp: "添加 MCP",
  editMcp: "编辑 MCP",
  newMcp: "新建 MCP",
  name: "名称",
  namePlaceholder: "例如 context7",
  transport: "传输方式",
  command: "命令",
  commandPlaceholder: "例如 npx",
  args: "参数（每行一项）",
  url: "服务 URL",
  enabled: "启用",
  sensitive: "含敏感字段",
  sensitiveKept: "未显示的环境变量、请求头和扩展字段会原样保留",
  saveMcp: "保存 MCP",
  cancel: "取消",
  delete: "删除",
  deleteConfirm: "确认删除 {name}？",
  showSecrets: "显示敏感值",
  hiddenHint: "敏感值已遮挡。开启“显示敏感值”后才能编辑原始 TOML。",
  validate: "校验 TOML",
  valid: "TOML 格式正确",
  saveRaw: "保存原始配置",
  restore: "恢复上一版本",
  restored: "已恢复上一版本；恢复前的内容现在仍可撤销",
  noBackup: "还没有可恢复的备份",
  rawDirty: "原始配置有未保存修改",
};

const EN_COPY: Record<keyof typeof ZH_COPY, string> = {
  loading: "Reading Codex configuration…",
  file: "Configuration file",
  openFolder: "Open folder",
  fileMissing: "config.toml will be created on save",
  backupReady: "Previous version can be restored",
  invalid: "The current TOML is invalid. Repair it in Advanced.",
  running: "Codex is running. Changes can be saved; restart Codex to apply them.",
  basic: "Basic",
  multi: "Multi-model",
  mcp: "MCP",
  advanced: "Advanced",
  model: "Model",
  modelPlaceholder: "For example, gpt-5.6-sol",
  fetchModels: "Fetch models",
  modelsFetched: "Fetched {count} models",
  provider: "Provider key",
  providerPlaceholder: "For example, osir",
  providers: "Configured providers",
  providerCount: "{count}",
  newProvider: "New provider",
  providerSelected: "Active",
  providerDetails: "Active provider details",
  recommended: "Recommended",
  recommendedProvider: "Recommended provider",
  baseUrl: "Base URL",
  credentials: "API credentials",
  authFile: "Credential file",
  apiKey: "API Key",
  imageApiKey: "Image generation API Key",
  imageApiKeyPlaceholder: "Enter the independent image API Key",
  imageApiKeyHint: "Saved to ~/.codex/imagegen-relay.json and installs the imagegen-relay skill; config.toml is untouched.",
  imageModel: "Image model",
  imageBaseUrl: "Image API Base URL",
  fetchImageModels: "Fetch image models",
  imageModelPlaceholder: "Defaults to gpt-image-2; fetch models to choose",
  saveImageApiKey: "Save and install skill",
  deleteImageApiKey: "Delete image API Key",
  apiKeyPlaceholder: "Enter a new API Key",
  apiKeyConfigured: "Configured",
  apiKeyMissing: "Not configured",
  apiKeyHint: "Saved keys are never shown again",
  showApiKey: "Show the API Key being entered",
  saveApiKey: "Save API Key",
  apiKeySaved: "API Key saved securely",
  deleteApiKey: "Delete API Key",
  deleteApiKeyConfirm: "Delete the API Key from auth.json?",
  apiKeyDeleted: "API Key deleted",
  reasoning: "Reasoning effort",
  personality: "Personality",
  goalMode: "Goal Mode",
  disableResponseStorage: "Disable response storage",
  imageGenerationCompatibility: "Third-party relay image mode",
  imageGenerationCompatibilityHint: "When enabled, image requests use the independent image API skill while chat keeps the regular API Key. Restart Codex afterward.",
  executionAccess: "Execution access",
  approvalPolicy: "Approval policy",
  sandboxMode: "Sandbox mode",
  dangerousCombination: "This combination lets Codex access all system files and run commands without confirmation. Use it only for tasks you fully trust.",
  automatic: "Use Codex default",
  saveBasic: "Save basic configuration",
  saved: "Configuration saved with a previous-version backup",
  savedRunning: "Configuration saved; restart Codex to apply it",
  emptyMcp: "No MCP servers configured",
  addMcp: "Add MCP",
  editMcp: "Edit MCP",
  newMcp: "New MCP",
  name: "Name",
  namePlaceholder: "For example, context7",
  transport: "Transport",
  command: "Command",
  commandPlaceholder: "For example, npx",
  args: "Arguments (one per line)",
  url: "Service URL",
  enabled: "Enabled",
  sensitive: "Contains sensitive fields",
  sensitiveKept: "Hidden environment, header, and extension fields will be preserved",
  saveMcp: "Save MCP",
  cancel: "Cancel",
  delete: "Delete",
  deleteConfirm: "Delete {name}?",
  showSecrets: "Show sensitive values",
  hiddenHint: "Sensitive values are masked. Enable “Show sensitive values” to edit raw TOML.",
  validate: "Validate TOML",
  valid: "TOML is valid",
  saveRaw: "Save raw configuration",
  restore: "Restore previous version",
  restored: "Previous version restored; the pre-restore content remains undoable",
  noBackup: "No backup is available yet",
  rawDirty: "Raw configuration has unsaved changes",
};

function mcpDraft(server?: CodexMcpServer): McpDraft {
  return {
    originalName: server?.name ?? null,
    name: server?.name ?? "",
    enabled: server?.enabled ?? true,
    transport: server?.transport ?? "stdio",
    command: server?.command ?? "",
    args: server?.args ?? [],
    argsText: server?.args.join("\n") ?? "",
    url: server?.url ?? "",
  };
}

function serverInput(draft: McpDraft): CodexMcpServerInput {
  return {
    originalName: draft.originalName,
    name: draft.name.trim(),
    enabled: draft.enabled,
    transport: draft.transport,
    command: draft.transport === "stdio" ? draft.command?.trim() || null : null,
    args:
      draft.transport === "stdio"
        ? draft.argsText
            .split("\n")
            .map((item) => item.trim())
            .filter(Boolean)
        : [],
    url: draft.transport === "http" ? draft.url?.trim() || null : null,
  };
}

export function CodexConfig({ onBack }: { onBack: () => void }) {
  const { t, lang } = useI18n();
  const copy = lang === "zh-CN" || lang === "zh-TW" ? ZH_COPY : EN_COPY;
  const [tab, setTab] = useState<ConfigTab>("basic");
  const [report, setReport] = useState<CodexConfigReport | null>(null);
  const [basic, setBasic] = useState<CodexBasicConfigInput>(EMPTY_BASIC);
  const [rawDraft, setRawDraft] = useState("");
  const [showSecrets, setShowSecrets] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [imageApiKey, setImageApiKey] = useState("");
  const [imageModel, setImageModel] = useState("gpt-image-2");
  const [imageBaseUrl, setImageBaseUrl] = useState("");
  const [imageModels, setImageModels] = useState<string[]>([]);
  const [showApiKey, setShowApiKey] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsBaseUrl, setModelsBaseUrl] = useState<string | null>(null);
  const [draft, setDraft] = useState<McpDraft | null>(null);
  const [deleteName, setDeleteName] = useState<string | null>(null);
  const [deleteApiKeyConfirm, setDeleteApiKeyConfirm] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const applyReport = useCallback((next: CodexConfigReport) => {
    setReport(next);
    setImageModel(next.imageGenerationModel || "gpt-image-2");
    setImageBaseUrl(next.imageGenerationBaseUrl || "");
    setBasic({
      model: next.model || "gpt-5.6-sol",
      provider: next.provider,
      baseUrl: next.baseUrl,
      reasoningEffort: next.reasoningEffort,
      personality: next.personality,
      approvalPolicy: next.approvalPolicy,
      sandboxMode: next.sandboxMode,
      disableResponseStorage: next.disableResponseStorage,
      goalMode: next.goalMode,
      imageGenerationCompatibility: next.imageGenerationCompatibility ?? false,
    });
    setRawDraft(next.raw);
    setDraft(null);
    setDeleteName(null);
    setDeleteApiKeyConfirm(false);
  }, []);

  const load = useCallback(async () => {
    setBusy("load");
    setError(null);
    try {
      applyReport(await managerApi.codexConfigGet());
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }, [applyReport]);

  useEffect(() => {
    void load();
  }, [load]);

  const run = async (
    kind: string,
    action: () => Promise<CodexConfigReport>,
    success: string,
  ) => {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      applyReport(await action());
      setNotice(success);
      return true;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const saveBasic = () =>
    run("basic", () => managerApi.codexConfigSaveBasic(basic), report?.codexRunning ? copy.savedRunning : copy.saved);

  const fetchModels = async () => {
    setBusy("models");
    setError(null);
    setNotice(null);
    try {
      const fetched = await managerApi.codexConfigFetchModels(basic.baseUrl);
      setModels(fetched);
      setModelsBaseUrl(basic.baseUrl);
      setNotice(copy.modelsFetched.replace("{count}", String(fetched.length)));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const fetchImageModels = async () => {
    setBusy("image-models");
    setError(null);
    setNotice(null);
    try {
      const fetched = await managerApi.codexConfigFetchImageModels();
      setImageModels(fetched);
      setNotice(copy.modelsFetched.replace("{count}", String(fetched.length)));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const saveMcp = (input: CodexMcpServerInput) =>
    run("mcp", () => managerApi.codexConfigUpsertMcp(input), report?.codexRunning ? copy.savedRunning : copy.saved);

  const saveApiKey = async () => {
    const saved = await run(
      "api-key",
      () => managerApi.codexConfigSetApiKey(apiKey),
      report?.codexRunning ? copy.savedRunning : copy.apiKeySaved,
    );
    if (saved) {
      setApiKey("");
      setShowApiKey(false);
    }
  };

  const saveImageApiKey = async () => {
    const saved = await run(
      "image-api-key",
      () => managerApi.codexConfigSetImageGenerationApiKey(imageApiKey, imageModel, imageBaseUrl),
      report?.codexRunning ? copy.savedRunning : copy.apiKeySaved,
    );
    if (saved) setImageApiKey("");
  };

  const toggleMcp = (server: CodexMcpServer, enabled: boolean) => {
    void saveMcp({
      ...serverInput(mcpDraft(server)),
      enabled,
    });
  };

  const validateRaw = async () => {
    setBusy("validate");
    setError(null);
    setNotice(null);
    try {
      const result = await managerApi.codexConfigValidate(rawDraft);
      if (result.valid) setNotice(copy.valid);
      else setError(result.error ?? copy.invalid);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const rawDirty = Boolean(report && rawDraft !== report.raw);
  const locked = Boolean(busy);
  const saveNotice = report?.codexRunning ? copy.savedRunning : copy.saved;
  const modelsReady = modelsBaseUrl === basic.baseUrl && models.length > 0;
  const providerProfiles = useMemo(() => {
    if (!report) return [];
    return [
      {
        id: "osir",
        name: "OSIR",
        baseUrl: "https://api.osirclaw.com/v1",
        wireApi: "responses",
      },
      ...report.providers.filter((profile) => profile.id !== "osir"),
    ];
  }, [report]);
  const selectedProvider = providerProfiles.find((profile) => profile.id === basic.provider);
  const dangerousCombination =
    basic.approvalPolicy === "never" && basic.sandboxMode === "danger-full-access";
  const shownRaw = showSecrets ? rawDraft : report?.redactedRaw ?? "";
  const tabs = useMemo(
    () => [
      { key: "basic", label: copy.basic },
      { key: "multi", label: copy.multi },
      { key: "mcp", label: `${copy.mcp}${report ? ` (${report.mcpServers.length})` : ""}` },
      { key: "advanced", label: copy.advanced },
    ],
    [copy, report],
  );

  return (
    <div className="pop config-pop">
      <NavBar title={t("nav.config")} onBack={onBack} />
      <div className="scroll view config-view">
        {busy === "load" && !report ? (
          <div className="banner info" role="status">
            <Icon name="loader" />
            <span>{copy.loading}</span>
          </div>
        ) : null}

        {report ? (
          <section className="config-filebar" aria-label={copy.file}>
            <div className="config-filecopy">
              <span className="config-eyebrow">{copy.file}</span>
              <strong className="config-path" title={report.path}>
                {report.path}
              </strong>
              <span className="config-filemeta">
                {report.exists ? (report.backupAvailable ? copy.backupReady : copy.noBackup) : copy.fileMissing}
              </span>
            </div>
            <button
              className="btn ghost icon-only"
              type="button"
              title={copy.openFolder}
              aria-label={copy.openFolder}
              onClick={() => void managerApi.openCodexHome().catch((cause) => setError(errorMessage(cause)))}
            >
              <Icon name="folder" />
            </button>
          </section>
        ) : null}

        {report?.codexRunning ? (
          <div className="banner warn" role="status">
            <Icon name="alert" />
            <span>{copy.running}</span>
          </div>
        ) : null}
        {report?.parseError ? (
          <button className="banner err config-error-jump" type="button" onClick={() => setTab("advanced")}>
            <Icon name="alert" />
            <span>{copy.invalid}</span>
            <Icon name="chevron" />
          </button>
        ) : null}
        {error ? (
          <div className="banner err" role="alert">
            <Icon name="alert" />
            <span>{error}</span>
          </div>
        ) : null}
        {notice ? (
          <div className="banner ok" role="status">
            <Icon name="check" />
            <span>{notice}</span>
          </div>
        ) : null}

        <Segmented
          items={tabs}
          value={tab}
          onChange={(next) => setTab(next as ConfigTab)}
          ariaLabel={t("nav.config")}
        />

        {report && tab === "basic" ? (
          <section className="config-panel" aria-label={copy.basic}>
            <div className="config-basic-layout">
              <aside className="config-provider-sidebar" aria-label={copy.providers}>
                <div className="config-sectionbar">
                  <span className="config-eyebrow">{copy.providers}</span>
                  <span className="config-sectioncount">
                    {copy.providerCount.replace("{count}", String(providerProfiles.length))}
                  </span>
                </div>
                <div className="config-provider-cards">
                  {providerProfiles.map((profile) => {
                    const active = basic.provider === profile.id;
                    const recommended = profile.id === "osir";
                    return (
                      <button
                        className={`config-provider-card${active ? " active" : ""}${recommended ? " recommended" : ""}`}
                        type="button"
                        key={profile.id}
                        disabled={locked}
                        aria-pressed={active}
                        aria-label={`${profile.name || profile.id} ${profile.id} ${profile.baseUrl || copy.automatic}${recommended ? ` ${copy.recommendedProvider}` : ""}`}
                        onClick={() =>
                          setBasic((current) => ({
                            ...current,
                            provider: profile.id,
                            baseUrl: profile.baseUrl,
                          }))
                        }
                      >
                        <span className="config-provider-card-copy">
                          <strong>{profile.name || profile.id}</strong>
                          <span className="mono">{profile.id}</span>
                          <small>{profile.baseUrl || copy.automatic}</small>
                        </span>
                        <span className="config-provider-card-meta">
                          {recommended ? (
                            <span className="config-provider-recommended" title={copy.recommendedProvider}>
                              <Icon name="star" />
                              <span>{copy.recommendedProvider}</span>
                            </span>
                          ) : null}
                          {active ? <span className="config-provider-active">{copy.providerSelected}</span> : null}
                        </span>
                      </button>
                    );
                  })}
                  <button
                    className="config-provider-card new"
                    type="button"
                    disabled={locked}
                    onClick={() =>
                      setBasic((current) => ({ ...current, provider: "", baseUrl: "" }))
                    }
                  >
                    <Icon name="plus" />
                    <span>{copy.newProvider}</span>
                  </button>
                </div>
              </aside>

              <section className="config-provider-details" aria-label={copy.providerDetails}>
                <div className="config-detail-head">
                  <div className="config-detail-copy">
                    <span className="config-eyebrow">{copy.providerDetails}</span>
                    <strong>{selectedProvider?.name || basic.provider || copy.newProvider}</strong>
                    <span className="config-path" title={basic.baseUrl || undefined}>
                      {basic.baseUrl || copy.automatic}
                    </span>
                  </div>
                </div>

                <div className="config-grid">
              <div className="config-field">
                <span>{copy.model}</span>
                <div className="config-model-control">
                  {modelsReady ? (
                    <select
                      className="input config-select mono"
                      value={basic.model}
                      disabled={locked}
                      aria-label={copy.model}
                      onChange={(event) => setBasic({ ...basic, model: event.target.value })}
                    >
                      {!models.includes(basic.model) ? <option value={basic.model}>{basic.model}（当前）</option> : null}
                      {models.map((model) => <option key={model} value={model}>{model}</option>)}
                    </select>
                  ) : (
                    <input
                      className="input mono"
                      value={basic.model}
                      disabled={locked}
                      aria-label={copy.model}
                      placeholder={copy.modelPlaceholder}
                      onChange={(event) => setBasic({ ...basic, model: event.target.value })}
                    />
                  )}
                  <button
                    className="btn ghost config-fetch-models"
                    type="button"
                    title={copy.fetchModels}
                    aria-label={copy.fetchModels}
                    disabled={locked}
                    onClick={() => void fetchModels()}
                  >
                    <Icon name={busy === "models" ? "loader" : "refresh"} />
                    <span>{copy.fetchModels}</span>
                  </button>
                </div>
              </div>
              <label className="config-field">
                <span>{copy.provider}</span>
                <input
                  className="input mono"
                  value={basic.provider}
                  disabled={locked}
                  list="codex-provider-options"
                  placeholder={copy.providerPlaceholder}
                  onChange={(event) => setBasic({ ...basic, provider: event.target.value })}
                />
                <datalist id="codex-provider-options">
                  {providerProfiles.map((profile) => <option key={profile.id} value={profile.id} />)}
                </datalist>
              </label>
              <label className="config-field config-field-wide">
                <span>{copy.baseUrl}</span>
                <input
                  className="input mono"
                  inputMode="url"
                  value={basic.baseUrl}
                  disabled={locked}
                  placeholder="https://api.example.com/v1"
                  onChange={(event) => setBasic({ ...basic, baseUrl: event.target.value })}
                />
              </label>
              <label className="config-field">
                <span>{copy.reasoning}</span>
                <select
                  className="input config-select"
                  value={basic.reasoningEffort}
                  disabled={locked}
                  onChange={(event) => setBasic({ ...basic, reasoningEffort: event.target.value })}
                >
                  <option value="">{copy.automatic}</option>
                  <option value="minimal">Minimal</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                  <option value="xhigh">XHigh</option>
                </select>
              </label>
              <label className="config-field">
                <span>{copy.personality}</span>
                <select
                  className="input config-select"
                  value={basic.personality}
                  disabled={locked}
                  onChange={(event) => setBasic({ ...basic, personality: event.target.value })}
                >
                  <option value="">{copy.automatic}</option>
                  <option value="none">none</option>
                  <option value="friendly">friendly</option>
                  <option value="pragmatic">pragmatic</option>
                </select>
              </label>
            </div>

            <div className="config-switches">
              <div className="config-toggle-row">
                <span>{copy.goalMode}</span>
                <Toggle
                  checked={basic.goalMode}
                  disabled={locked}
                  ariaLabel={copy.goalMode}
                  onChange={(goalMode) => setBasic({ ...basic, goalMode })}
                />
              </div>
              <div className="config-toggle-row config-toggle-row-wide">
                <div>
                  <span>{copy.imageGenerationCompatibility}</span>
                  <small>{copy.imageGenerationCompatibilityHint}</small>
                </div>
                <Toggle
                  checked={basic.imageGenerationCompatibility}
                  disabled={locked || !basic.provider || !basic.baseUrl || !report.imageGenerationApiKeyConfigured}
                  ariaLabel={copy.imageGenerationCompatibility}
                  onChange={(imageGenerationCompatibility) => setBasic({ ...basic, imageGenerationCompatibility })}
                />
              </div>
              <div className="config-toggle-row">
                <span>{copy.disableResponseStorage}</span>
                <Toggle
                  checked={basic.disableResponseStorage}
                  disabled={locked}
                  ariaLabel={copy.disableResponseStorage}
                  onChange={(disableResponseStorage) => setBasic({ ...basic, disableResponseStorage })}
                />
              </div>
            </div>

            <div className="config-access">
              <span className="config-eyebrow">{copy.executionAccess}</span>
              <div className="config-grid">
                <label className="config-field">
                  <span>{copy.approvalPolicy}</span>
                  <select
                    className="input config-select mono"
                    value={basic.approvalPolicy}
                    disabled={locked}
                    onChange={(event) => setBasic({ ...basic, approvalPolicy: event.target.value })}
                  >
                    <option value="">{copy.automatic}</option>
                    <option value="untrusted">untrusted</option>
                    <option value="on-request">on-request</option>
                    <option value="never">never</option>
                  </select>
                </label>
                <label className="config-field">
                  <span>{copy.sandboxMode}</span>
                  <select
                    className="input config-select mono"
                    value={basic.sandboxMode}
                    disabled={locked}
                    onChange={(event) => setBasic({ ...basic, sandboxMode: event.target.value })}
                  >
                    <option value="">{copy.automatic}</option>
                    <option value="read-only">read-only</option>
                    <option value="workspace-write">workspace-write</option>
                    <option value="danger-full-access">danger-full-access</option>
                  </select>
                </label>
              </div>
              {dangerousCombination ? (
                <div className="config-danger-note" role="alert">
                  <Icon name="alert" />
                  <span>{copy.dangerousCombination}</span>
                </div>
              ) : null}
            </div>

            <div className="config-actions">
              <button
                className="btn primary"
                type="button"
                disabled={locked || Boolean(report.parseError)}
                onClick={() => void saveBasic()}
              >
                <Icon name={busy === "basic" ? "loader" : "check"} />
                {copy.saveBasic}
              </button>
            </div>

            <section className="config-auth" aria-label={copy.credentials}>
              <div className="config-auth-head">
                <div className="config-auth-copy">
                  <span className="config-eyebrow">{copy.authFile}</span>
                  <strong>{copy.apiKey}</strong>
                  <span className="config-path" title={report.authPath}>
                    {report.authPath}
                  </span>
                </div>
                <span
                  className={`config-auth-status${report.apiKeyConfigured ? " configured" : ""}`}
                >
                  <Icon name={report.apiKeyConfigured ? "check" : "alert"} />
                  {report.apiKeyConfigured ? copy.apiKeyConfigured : copy.apiKeyMissing}
                </span>
              </div>
              {report.authError ? (
                <div className="banner err config-auth-error" role="alert">
                  <Icon name="alert" />
                  <span>{report.authError}</span>
                </div>
              ) : null}
              <label className="config-field">
                <span>{copy.apiKey}</span>
                <input
                  className="input mono"
                  type={showApiKey ? "text" : "password"}
                  name="codex-api-key"
                  autoComplete="new-password"
                  spellCheck={false}
                  value={apiKey}
                  disabled={locked}
                  placeholder={copy.apiKeyPlaceholder}
                  onChange={(event) => setApiKey(event.target.value)}
                />
              </label>
              <div className="config-auth-meta">
                <span>{copy.apiKeyHint}</span>
                <div className="config-auth-reveal">
                  <span>{copy.showApiKey}</span>
                  <Toggle
                    checked={showApiKey}
                    disabled={locked || !apiKey}
                    ariaLabel={copy.showApiKey}
                    onChange={setShowApiKey}
                  />
                </div>
              </div>
              <div className="config-actions config-actions-wrap">
                {report.apiKeyConfigured ? (
                  <button
                    className="btn ghost danger"
                    type="button"
                    disabled={locked}
                    onClick={() => setDeleteApiKeyConfirm(true)}
                  >
                    <Icon name="trash" />
                    {copy.deleteApiKey}
                  </button>
                ) : null}
                <button
                  className="btn primary"
                  type="button"
                  disabled={locked || !apiKey.trim()}
                  onClick={() => void saveApiKey()}
                >
                  <Icon name={busy === "api-key" ? "loader" : "shield"} />
                  {copy.saveApiKey}
                </button>
              </div>
            </section>
            <section className="config-auth" aria-label={copy.imageApiKey}>
              <div className="config-auth-head">
                <div className="config-auth-copy">
                  <span className="config-eyebrow">{copy.imageApiKey}</span>
                  <strong>{imageBaseUrl || copy.imageApiKey}</strong>
                  <span className="config-path">~/.codex/imagegen-relay.json</span>
                </div>
                <span className={`config-auth-status${report.imageGenerationApiKeyConfigured ? " configured" : ""}`}>
                  <Icon name={report.imageGenerationApiKeyConfigured ? "check" : "alert"} />
                  {report.imageGenerationApiKeyConfigured ? copy.apiKeyConfigured : copy.apiKeyMissing}
                </span>
              </div>
              <label className="config-field">
                <span>{copy.imageBaseUrl}</span>
                <input
                  className="input mono"
                  inputMode="url"
                  value={imageBaseUrl}
                  disabled={locked}
                  placeholder="https://api.example.com/v1"
                  onChange={(event) => setImageBaseUrl(event.target.value)}
                />
              </label>
              <div className="config-field">
                <span>{copy.imageModel}</span>
                <div className="config-model-control">
                  {imageModels.length ? (
                    <select className="input config-select mono" value={imageModel} disabled={locked} onChange={(event) => setImageModel(event.target.value)}>
                      {!imageModels.includes(imageModel) ? <option value={imageModel}>{imageModel}（当前）</option> : null}
                      {imageModels.map((model) => <option key={model} value={model}>{model}</option>)}
                    </select>
                  ) : (
                    <input className="input mono" value={imageModel} disabled={locked} placeholder={copy.imageModelPlaceholder} onChange={(event) => setImageModel(event.target.value)} />
                  )}
                  <button className="btn ghost config-fetch-models" type="button" title={copy.fetchImageModels} aria-label={copy.fetchImageModels} disabled={locked || !report.imageGenerationApiKeyConfigured} onClick={() => void fetchImageModels()}>
                    <Icon name={busy === "image-models" ? "loader" : "refresh"} /><span>{copy.fetchImageModels}</span>
                  </button>
                </div>
                <datalist id="image-model-options">
                  {imageModels.map((model) => <option key={model} value={model} />)}
                </datalist>
                <small>{copy.imageModelPlaceholder}</small>
              </div>
              <label className="config-field">
                <span>{copy.imageApiKey}</span>
                <input
                  className="input mono"
                  type="password"
                  autoComplete="new-password"
                  spellCheck={false}
                  value={imageApiKey}
                  disabled={locked}
                  placeholder={copy.imageApiKeyPlaceholder}
                  onChange={(event) => setImageApiKey(event.target.value)}
                />
              </label>
              <div className="config-auth-meta"><span>{copy.imageApiKeyHint}</span></div>
              <div className="config-actions config-actions-wrap">
                {report.imageGenerationApiKeyConfigured ? (
                  <button className="btn ghost danger" type="button" disabled={locked} onClick={() => void run("delete-image-api-key", () => managerApi.codexConfigDeleteImageGenerationApiKey(), copy.saved)}>
                    <Icon name="trash" />{copy.deleteImageApiKey}
                  </button>
                ) : null}
                <button className="btn primary" type="button" disabled={locked || !imageApiKey.trim() || !imageBaseUrl.trim()} onClick={() => void saveImageApiKey()}>
                  <Icon name={busy === "image-api-key" ? "loader" : "shield"} />{copy.saveImageApiKey}
                </button>
              </div>
            </section>
              </section>
            </div>
          </section>
        ) : null}

        {report && tab === "multi" ? <OpenCodexPrototype /> : null}

        {report && tab === "mcp" ? (
          <section className="config-panel" aria-label={copy.mcp}>
            {!draft ? (
              <>
                <div className="config-sectionbar">
                  <span className="config-sectioncount">{report.mcpServers.length}</span>
                  <button
                    className="btn ghost sm"
                    type="button"
                    disabled={locked || Boolean(report.parseError)}
                    onClick={() => setDraft(mcpDraft())}
                  >
                    <Icon name="plus" />
                    {copy.addMcp}
                  </button>
                </div>
                {report.mcpServers.length ? (
                  <div className="config-mcp-list">
                    {report.mcpServers.map((server) => (
                      <div className="config-mcp" key={server.name}>
                        <button
                          className="config-mcp-main"
                          type="button"
                          disabled={Boolean(busy)}
                          onClick={() => setDraft(mcpDraft(server))}
                        >
                          <span className={`config-transport ${server.transport}`}>
                            {server.transport}
                          </span>
                          <span className="config-mcp-copy">
                            <strong>{server.name}</strong>
                            <span className="mono">
                              {server.transport === "stdio"
                                ? [server.command, ...server.args].filter(Boolean).join(" ")
                                : server.url}
                            </span>
                          </span>
                          {server.hasSensitiveValues ? (
                            <span className="tag">{copy.sensitive}</span>
                          ) : null}
                        </button>
                        <Toggle
                          checked={server.enabled}
                          disabled={locked}
                          ariaLabel={`${copy.enabled} ${server.name}`}
                          onChange={(enabled) => toggleMcp(server, enabled)}
                        />
                        <button
                          className="btn ghost danger icon-only"
                          type="button"
                          title={copy.delete}
                          aria-label={`${copy.delete} ${server.name}`}
                          disabled={locked}
                          onClick={() => setDeleteName(server.name)}
                        >
                          <Icon name="trash" />
                        </button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="config-empty">{copy.emptyMcp}</div>
                )}
              </>
            ) : (
              <div className="config-mcp-editor">
                <div className="config-sectionbar">
                  <strong>{draft.originalName ? copy.editMcp : copy.newMcp}</strong>
                  <button className="linkbtn" type="button" disabled={Boolean(busy)} onClick={() => setDraft(null)}>
                    {copy.cancel}
                  </button>
                </div>
                <div className="config-grid">
                  <label className="config-field config-field-wide">
                    <span>{copy.name}</span>
                    <input
                      className="input mono"
                      value={draft.name}
                      disabled={locked}
                      placeholder={copy.namePlaceholder}
                      onChange={(event) => setDraft({ ...draft, name: event.target.value })}
                    />
                  </label>
                  <div className="config-field config-field-wide">
                    <span>{copy.transport}</span>
                    <Segmented
                      items={[
                        { key: "stdio", label: "stdio" },
                        { key: "http", label: "HTTP" },
                      ]}
                      value={draft.transport}
                      onChange={(transport) =>
                        setDraft({ ...draft, transport: transport as CodexMcpTransport })
                      }
                      ariaLabel={copy.transport}
                    />
                  </div>
                  {draft.transport === "stdio" ? (
                    <>
                      <label className="config-field config-field-wide">
                        <span>{copy.command}</span>
                        <input
                          className="input mono"
                          value={draft.command ?? ""}
                          disabled={locked}
                          placeholder={copy.commandPlaceholder}
                          onChange={(event) => setDraft({ ...draft, command: event.target.value })}
                        />
                      </label>
                      <label className="config-field config-field-wide">
                        <span>{copy.args}</span>
                        <textarea
                          className="input mono config-args"
                          value={draft.argsText}
                          disabled={locked}
                          onChange={(event) => setDraft({ ...draft, argsText: event.target.value })}
                        />
                      </label>
                    </>
                  ) : (
                    <label className="config-field config-field-wide">
                      <span>{copy.url}</span>
                      <input
                        className="input mono"
                        inputMode="url"
                        value={draft.url ?? ""}
                        disabled={locked}
                        placeholder="https://example.com/mcp"
                        onChange={(event) => setDraft({ ...draft, url: event.target.value })}
                      />
                    </label>
                  )}
                </div>
                <div className="config-toggle-row">
                  <span>{copy.enabled}</span>
                  <Toggle
                    checked={draft.enabled}
                    disabled={locked}
                    ariaLabel={copy.enabled}
                    onChange={(enabled) => setDraft({ ...draft, enabled })}
                  />
                </div>
                {draft.originalName && report.mcpServers.find((item) => item.name === draft.originalName)?.hasSensitiveValues ? (
                  <div className="config-preserve-note">
                    <Icon name="shield" />
                    <span>{copy.sensitiveKept}</span>
                  </div>
                ) : null}
                <div className="config-actions">
                  <button
                    className="btn primary"
                    type="button"
                    disabled={locked || !draft.name.trim()}
                    onClick={() => void saveMcp(serverInput(draft))}
                  >
                    <Icon name={busy === "mcp" ? "loader" : "check"} />
                    {copy.saveMcp}
                  </button>
                </div>
              </div>
            )}
          </section>
        ) : null}

        {report && tab === "advanced" ? (
          <section className="config-panel config-advanced" aria-label={copy.advanced}>
            <div className="config-toggle-row">
              <span>{copy.showSecrets}</span>
              <Toggle
                checked={showSecrets}
                ariaLabel={copy.showSecrets}
                onChange={setShowSecrets}
              />
            </div>
            {!showSecrets ? <div className="config-mask-note">{copy.hiddenHint}</div> : null}
            <textarea
              className="config-editor"
              value={shownRaw}
              readOnly={!showSecrets}
              spellCheck={false}
              aria-label="config.toml"
              onChange={(event) => setRawDraft(event.target.value)}
            />
            {rawDirty ? <div className="config-dirty">{copy.rawDirty}</div> : null}
            <div className="config-actions config-actions-wrap">
              <button
                className="btn ghost"
                type="button"
                disabled={Boolean(busy)}
                onClick={() => void validateRaw()}
              >
                <Icon name={busy === "validate" ? "loader" : "check"} />
                {copy.validate}
              </button>
              <button
                className="btn ghost"
                type="button"
                disabled={locked || !report.backupAvailable}
                title={!report.backupAvailable ? copy.noBackup : undefined}
                onClick={() =>
                  void run("restore", () => managerApi.codexConfigRestoreBackup(), saveNotice)
                }
              >
                <Icon name={busy === "restore" ? "loader" : "refresh"} />
                {copy.restore}
              </button>
              <button
                className="btn primary"
                type="button"
                disabled={locked || !showSecrets || !rawDirty}
                onClick={() =>
                  void run("raw", () => managerApi.codexConfigSaveRaw(rawDraft), saveNotice)
                }
              >
                <Icon name={busy === "raw" ? "loader" : "check"} />
                {copy.saveRaw}
              </button>
            </div>
          </section>
        ) : null}

        {deleteName ? (
          <div className="config-confirm" role="dialog" aria-modal="true">
            <strong>{copy.deleteConfirm.replace("{name}", deleteName)}</strong>
            <div className="config-actions">
              <button className="btn ghost" type="button" onClick={() => setDeleteName(null)}>
                {copy.cancel}
              </button>
              <button
                className="btn danger"
                type="button"
                disabled={Boolean(busy)}
                onClick={() =>
                  void run(
                    "delete",
                    () => managerApi.codexConfigDeleteMcp(deleteName),
                    copy.saved,
                  )
                }
              >
                <Icon name="trash" />
                {copy.delete}
              </button>
            </div>
          </div>
        ) : null}
        {deleteApiKeyConfirm ? (
          <div className="config-confirm" role="dialog" aria-modal="true">
            <strong>{copy.deleteApiKeyConfirm}</strong>
            <div className="config-actions">
              <button
                className="btn ghost"
                type="button"
                onClick={() => setDeleteApiKeyConfirm(false)}
              >
                {copy.cancel}
              </button>
              <button
                className="btn danger"
                type="button"
                disabled={Boolean(busy)}
                onClick={() =>
                  void run(
                    "delete-api-key",
                    () => managerApi.codexConfigDeleteApiKey(),
                    copy.apiKeyDeleted,
                  )
                }
              >
                <Icon name="trash" />
                {copy.deleteApiKey}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
