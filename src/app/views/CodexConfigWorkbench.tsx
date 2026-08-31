import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { errorMessage, managerApi } from "../../services/managerApi";
import type {
  CodexBasicConfigInput,
  CodexConfigReport,
  CodexMcpServer,
  CodexMcpServerInput,
  CodexMcpTransport,
  CodexProviderProfile,
  OpenCodexStatus,
} from "../../shared/types";
import { NavBar, Segmented, Toggle } from "../components";
import { Icon, type IconName } from "../icons";
import { useI18n } from "../i18n";
import { Sheet } from "../Sheet";
import { OpenCodexPrototype } from "./OpenCodexPrototype";

type ConfigModule = "connections" | "behavior" | "tools" | "advanced";
type ConnectionView = "single" | "multi";
type DialogKind = "provider" | "behavior" | "credential" | "image" | "mcp" | "raw";
type ProviderHealth = "unknown" | "checking" | "connected" | "failed";

interface McpDraft extends CodexMcpServerInput {
  argsText: string;
}

type ConfirmState =
  | { kind: "discard"; returnTo: DialogKind }
  | { kind: "delete-mcp"; name: string }
  | { kind: "delete-api-key" }
  | { kind: "delete-image-api-key" }
  | { kind: "restore" }
  | { kind: "switch-default" }
  | { kind: "switch-multi" };

const EMPTY_BASIC: CodexBasicConfigInput = {
  model: "gpt-5.6-sol", provider: "", baseUrl: "", reasoningEffort: "",
  personality: "", approvalPolicy: "", sandboxMode: "",
  disableResponseStorage: false, goalMode: false, imageGenerationCompatibility: false,
};

const ZH_COPY = {
  loading: "正在读取 Codex 配置…", connections: "连接与模型", behavior: "Codex 行为", tools: "MCP 与工具", advanced: "高级与恢复",
  currentMode: "当前生效模式", singleMode: "默认配置", multiMode: "OpenCodex 多模型", notConfigured: "尚未完成配置",
  defaultConnection: "Codex 默认", defaultConfigFile: "默认配置文件", defaultConfigFileHint: "读取当前 Codex 配置文件并直接生效", selectProvider: "选择供应商", switchProvider: "切换为当前供应商", selectedProvider: "选中供应商", providerConnected: "连接成功", providerFailed: "连接失败", providerChecking: "检查中", providerUnknown: "未检查", modeSwitching: "正在切换配置模式…", backToDefault: "切回默认配置", enableDefault: "启用默认配置", enableMulti: "启用 OpenCodex 多模型", confirmDefault: "确认切换到默认配置？", confirmMulti: "确认启用 OpenCodex 多模型？", confirmDefaultBody: "切换前会验证默认模型可用性，并兼容迁移历史会话索引；验证失败时保持 OpenCodex 继续生效。", confirmMultiBody: "Codex 将切换到本机 OpenCodex 路由，并兼容迁移历史会话索引。对话正文不会移动或删除。", restartCodex: "重启 Codex", restartHint: "配置已切换；重启 Codex 即可生效。", defaultModeHint: "使用 Codex 配置文件中的供应商链",
  available: "可直接使用", needsRestart: "重启 Codex 后生效", connectionError: "配置需要修复", currentPath: "当前调用路径",
  currentModel: "当前模型", currentProvider: "当前供应商", credentials: "API 凭据", configured: "已配置", missing: "未配置",
  mcpCount: "MCP 服务", backup: "配置备份", backupReady: "可恢复", noBackup: "暂无备份",
  manageSingle: "配置单供应商", editConnection: "编辑连接", addProvider: "添加供应商", rotateKey: "管理 API Key",
  providerList: "供应商连接", providerListHint: "选择一项进行编辑；只有当前连接会写入 Codex 生效配置。", recommended: "推荐", current: "当前使用",
  provider: "供应商标识", providerPlaceholder: "例如 osir", baseUrl: "Base URL", model: "模型", modelPlaceholder: "例如 gpt-5.6-sol",
  fetchModels: "获取模型", modelsFetched: "已获取 {count} 个模型", apiKey: "API Key", apiKeyPlaceholder: "输入新的 API Key",
  apiKeyHint: "已保存的密钥不会回显；留空表示保留现有凭据。", showApiKey: "显示正在输入的 API Key", chooseProvider: "选择供应商",
  configureConnection: "配置连接", chooseModel: "选择模型", step: "步骤", previous: "上一步", next: "下一步", saveAndEnable: "保存并启用",
  providerSaved: "单供应商连接已保存", connectionHelp: "保存前会检查必填字段，现有配置会保留备份。", behaviorSummary: "行为与权限摘要",
  editBehavior: "编辑行为设置", reasoning: "推理等级", personality: "Personality", goalMode: "Goal Mode", disableResponseStorage: "禁用响应存储",
  imageGenerationCompatibility: "第三方中转生图兼容模式", imageGenerationCompatibilityHint: "开启后图片请求使用独立图片 API；聊天继续使用普通 API Key。",
  approvalPolicy: "审批策略", sandboxMode: "沙箱模式", automatic: "跟随 Codex 默认", dangerousCombination: "当前组合允许 Codex 无需确认访问系统全部文件并执行命令。仅在完全信任当前任务时使用。",
  saveBehavior: "保存行为设置", behaviorSaved: "Codex 行为设置已保存", mcpServers: "MCP 服务器", mcpHint: "列表只展示状态；添加和编辑在独立弹窗中完成。",
  emptyMcp: "尚未配置 MCP 服务器", addMcp: "添加 MCP", editMcp: "编辑 MCP", newMcp: "新建 MCP", name: "名称", namePlaceholder: "例如 context7",
  transport: "传输方式", command: "命令", commandPlaceholder: "例如 npx", args: "参数（每行一项）", url: "服务 URL", enabled: "启用",
  sensitive: "含敏感字段", sensitiveKept: "未显示的环境变量、请求头和扩展字段会原样保留。", saveMcp: "保存 MCP", mcpSaved: "MCP 配置已保存",
  imageTool: "图片生成工具", imageToolHint: "独立管理图片模型与凭据，不影响聊天 API。", configureImage: "配置图片生成", imageApiKey: "生图 API Key",
  imageApiKeyPlaceholder: "输入独立图片 API Key", imageModel: "生图模型", imageBaseUrl: "生图 API Base URL", fetchImageModels: "获取生图模型",
  imageModelPlaceholder: "默认 gpt-image-2；可获取模型后选择", saveImageApiKey: "保存并安装技能", deleteImageApiKey: "删除生图 API Key", imageSaved: "图片生成配置已保存",
  diagnostics: "配置诊断", configFile: "配置文件", openFolder: "打开目录", parseStatus: "TOML 状态", valid: "格式正确", invalid: "当前 TOML 有错误，请在高级编辑中修复",
  codexStatus: "Codex 状态", running: "正在运行", stopped: "未运行", editRaw: "编辑原始配置", showSecrets: "显示并编辑敏感值",
  hiddenHint: "敏感值已遮挡。开启后才能编辑并保存原始 TOML。", validate: "校验 TOML", saveRaw: "保存原始配置", rawDirty: "原始配置有未保存修改",
  restore: "恢复上一版本", restored: "已恢复上一版本；恢复前的内容仍可撤销", saveApiKey: "保存 API Key", deleteApiKey: "删除 API Key",
  apiKeySaved: "API Key 已安全保存", apiKeyDeleted: "API Key 已删除", cancel: "取消", delete: "删除", confirmDeleteMcp: "确认删除 MCP “{name}”？",
  confirmDeleteApiKey: "确认删除 Codex API Key？删除后当前单供应商可能无法调用。", confirmDeleteImageKey: "确认删除图片生成 API Key？聊天 API 不受影响。",
  confirmRestore: "确认恢复上一版本配置？当前配置会先被保存为可撤销版本。", discardTitle: "放弃未保存的修改？", discardBody: "弹窗中的修改还没有保存。放弃后将恢复为当前已生效配置。",
  keepEditing: "继续编辑", discard: "放弃修改", saved: "配置已保存，并保留了上一版本备份", savedRunning: "配置已保存；重启 Codex 后生效",
} as const;

type Copy = { [K in keyof typeof ZH_COPY]: string };
const EN_COPY: Copy = {
  loading: "Reading Codex configuration…", connections: "Connections & models", behavior: "Codex behavior", tools: "MCP & tools", advanced: "Advanced & recovery",
  currentMode: "Active mode", singleMode: "Default configuration", multiMode: "OpenCodex multi-model", notConfigured: "Not configured", defaultConnection: "Codex default", defaultConfigFile: "Default config file", defaultConfigFileHint: "Read and apply the active Codex configuration file", selectProvider: "Choose provider", switchProvider: "Use this provider", selectedProvider: "Selected provider", providerConnected: "Connected", providerFailed: "Connection failed", providerChecking: "Checking", providerUnknown: "Not checked", modeSwitching: "Switching configuration mode…", backToDefault: "Back to default configuration", enableDefault: "Enable default configuration", enableMulti: "Enable OpenCodex multi-model", confirmDefault: "Switch to the default configuration?", confirmMulti: "Enable OpenCodex multi-model?", confirmDefaultBody: "Codex will restore the default provider from config.toml. Your OpenCodex configuration will be kept.", confirmMultiBody: "Codex will switch to the local OpenCodex routes. Existing conversation data will not be moved or deleted.", restartCodex: "Restart Codex", restartHint: "Configuration switched; restart Codex to apply it.", defaultModeHint: "Uses the provider chain from Codex config.toml", available: "Ready to use", needsRestart: "Restart Codex to apply", connectionError: "Configuration needs repair", currentPath: "Request path", currentModel: "Current model", currentProvider: "Current provider", credentials: "API credentials", configured: "Configured", missing: "Missing", mcpCount: "MCP services", backup: "Backup", backupReady: "Available", noBackup: "No backup", manageSingle: "Configure provider", editConnection: "Edit connection", addProvider: "Add provider", rotateKey: "Manage API Key", providerList: "Provider connections", providerListHint: "Select a provider to inspect it. Only the active provider is written to Codex.", recommended: "Recommended", current: "Active", provider: "Provider key", providerPlaceholder: "For example, osir", baseUrl: "Base URL", model: "Model", modelPlaceholder: "For example, gpt-5.6-sol", fetchModels: "Fetch models", modelsFetched: "Fetched {count} models", apiKey: "API Key", apiKeyPlaceholder: "Enter a new API Key", apiKeyHint: "Saved keys are never shown; leave blank to keep the current credential.", showApiKey: "Show the API Key being entered", chooseProvider: "Choose provider", configureConnection: "Configure connection", chooseModel: "Choose model", step: "Step", previous: "Previous", next: "Next", saveAndEnable: "Save and enable", providerSaved: "Default provider saved", connectionHelp: "Required fields are checked before save and the previous configuration is backed up.", behaviorSummary: "Behavior and permission summary", editBehavior: "Edit behavior", reasoning: "Reasoning effort", personality: "Personality", goalMode: "Goal Mode", disableResponseStorage: "Disable response storage", imageGenerationCompatibility: "Third-party image relay mode", imageGenerationCompatibilityHint: "Image requests use the independent image API while chat keeps the normal API Key.", approvalPolicy: "Approval policy", sandboxMode: "Sandbox mode", automatic: "Use Codex default", dangerousCombination: "This combination lets Codex access all system files and run commands without confirmation.", saveBehavior: "Save behavior", behaviorSaved: "Codex behavior saved", mcpServers: "MCP servers", mcpHint: "The list shows status only; add and edit in a focused dialog.", emptyMcp: "No MCP servers configured", addMcp: "Add MCP", editMcp: "Edit MCP", newMcp: "New MCP", name: "Name", namePlaceholder: "For example, context7", transport: "Transport", command: "Command", commandPlaceholder: "For example, npx", args: "Arguments (one per line)", url: "Service URL", enabled: "Enabled", sensitive: "Contains sensitive fields", sensitiveKept: "Hidden environment, header and extension fields are preserved.", saveMcp: "Save MCP", mcpSaved: "MCP configuration saved", imageTool: "Image generation", imageToolHint: "Manage the image model and credential separately from chat.", configureImage: "Configure image generation", imageApiKey: "Image API Key", imageApiKeyPlaceholder: "Enter the independent image API Key", imageModel: "Image model", imageBaseUrl: "Image API Base URL", fetchImageModels: "Fetch image models", imageModelPlaceholder: "Defaults to gpt-image-2; fetch models to choose", saveImageApiKey: "Save and install skill", deleteImageApiKey: "Delete image API Key", imageSaved: "Image generation configuration saved", diagnostics: "Configuration diagnostics", configFile: "Configuration file", openFolder: "Open folder", parseStatus: "TOML status", valid: "Valid", invalid: "The current TOML is invalid. Repair it in the advanced editor.", codexStatus: "Codex status", running: "Running", stopped: "Not running", editRaw: "Edit raw configuration", showSecrets: "Show and edit sensitive values", hiddenHint: "Sensitive values are masked. Enable editing before saving raw TOML.", validate: "Validate TOML", saveRaw: "Save raw configuration", rawDirty: "Raw configuration has unsaved changes", restore: "Restore previous version", restored: "Previous configuration restored; the pre-restore content remains undoable", saveApiKey: "Save API Key", deleteApiKey: "Delete API Key", apiKeySaved: "API Key saved securely", apiKeyDeleted: "API Key deleted", cancel: "Cancel", delete: "Delete", confirmDeleteMcp: "Delete MCP “{name}”?", confirmDeleteApiKey: "Delete the Codex API Key? The active provider may stop working.", confirmDeleteImageKey: "Delete the image API Key? Chat is not affected.", confirmRestore: "Restore the previous configuration? The current version remains undoable.", discardTitle: "Discard unsaved changes?", discardBody: "Changes in this dialog have not been saved. Discarding restores the active configuration.", keepEditing: "Keep editing", discard: "Discard changes", saved: "Configuration saved with a previous-version backup", savedRunning: "Configuration saved; restart Codex to apply it",
};

function basicFromReport(report: CodexConfigReport): CodexBasicConfigInput {
  return { model: report.model || "gpt-5.6-sol", provider: report.provider, baseUrl: report.baseUrl, reasoningEffort: report.reasoningEffort, personality: report.personality, approvalPolicy: report.approvalPolicy, sandboxMode: report.sandboxMode, disableResponseStorage: report.disableResponseStorage, goalMode: report.goalMode, imageGenerationCompatibility: report.imageGenerationCompatibility ?? false };
}

function mcpDraft(server?: CodexMcpServer): McpDraft {
  return { originalName: server?.name ?? null, name: server?.name ?? "", enabled: server?.enabled ?? true, transport: server?.transport ?? "stdio", command: server?.command ?? "", args: server?.args ?? [], argsText: server?.args.join("\n") ?? "", url: server?.url ?? "" };
}

function serverInput(draft: McpDraft): CodexMcpServerInput {
  return { originalName: draft.originalName, name: draft.name.trim(), enabled: draft.enabled, transport: draft.transport, command: draft.transport === "stdio" ? draft.command?.trim() || null : null, args: draft.transport === "stdio" ? draft.argsText.split("\n").map((item) => item.trim()).filter(Boolean) : [], url: draft.transport === "http" ? draft.url?.trim() || null : null };
}

function sameBasic(left: CodexBasicConfigInput, right: CodexBasicConfigInput, keys: Array<keyof CodexBasicConfigInput>) {
  return keys.every((key) => left[key] === right[key]);
}

function StatusBadge({ tone, icon, children }: { tone: "ok" | "warn" | "neutral" | "error"; icon: IconName; children: ReactNode }) {
  return <span className={"config-status-badge " + tone}><Icon name={icon} />{children}</span>;
}

function SummaryItem({ label, value, hint }: { label: string; value: ReactNode; hint?: string }) {
  return <div className="config-summary-item"><span>{label}</span><strong>{value}</strong>{hint ? <small>{hint}</small> : null}</div>;
}

function ConfigDialog({ open, eyebrow, title, titleId, onDismiss, wide = false, children, actions }: { open: boolean; eyebrow: string; title: string; titleId: string; onDismiss: () => void; wide?: boolean; children: ReactNode; actions: ReactNode }) {
  return <Sheet open={open} onDismiss={onDismiss} centeredInExpanded labelledBy={titleId} initialFocus="first"><div className={"config-dialog" + (wide ? " wide" : "")}><header className="config-dialog-head"><div><span className="config-eyebrow">{eyebrow}</span><h2 id={titleId}>{title}</h2></div><button className="iconbtn" type="button" aria-label="关闭" onClick={onDismiss}><Icon name="close" /></button></header><div className="config-dialog-body">{children}</div><footer className="config-dialog-actions">{actions}</footer></div></Sheet>;
}

export function CodexConfigWorkbench({ onBack }: { onBack: () => void }) {
  const { t, lang } = useI18n();
  const copy = lang === "zh-CN" || lang === "zh-TW" ? ZH_COPY : EN_COPY;
  const [module, setModule] = useState<ConfigModule>("connections");
  const [connectionView, setConnectionView] = useState<ConnectionView>("single");
  const [dialog, setDialog] = useState<DialogKind | null>(null);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);
  const [report, setReport] = useState<CodexConfigReport | null>(null);
  const [openCodex, setOpenCodex] = useState<OpenCodexStatus | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [providerHealth, setProviderHealth] = useState<Record<string, ProviderHealth>>({});
  const [basic, setBasic] = useState<CodexBasicConfigInput>(EMPTY_BASIC);
  const [dialogBaseline, setDialogBaseline] = useState({ provider: EMPTY_BASIC, behavior: EMPTY_BASIC, imageModel: "gpt-image-2", imageBaseUrl: "", raw: "", mcp: "" });
  const [providerStep, setProviderStep] = useState(1);
  const [providerApiKey, setProviderApiKey] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsBaseUrl, setModelsBaseUrl] = useState<string | null>(null);
  const [imageApiKey, setImageApiKey] = useState("");
  const [imageModel, setImageModel] = useState("gpt-image-2");
  const [imageBaseUrl, setImageBaseUrl] = useState("");
  const [imageModels, setImageModels] = useState<string[]>([]);
  const [draft, setDraft] = useState<McpDraft | null>(null);
  const [rawDraft, setRawDraft] = useState("");
  const [showSecrets, setShowSecrets] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [restartRequired, setRestartRequired] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  const applyReport = useCallback((next: CodexConfigReport) => {
    setReport(next);
    setBasic(basicFromReport(next));
    setRawDraft(next.raw);
    setImageModel(next.imageGenerationModel || "gpt-image-2");
    setImageBaseUrl(next.imageGenerationBaseUrl || "");
    setProviderApiKey("");
    setApiKey("");
    setImageApiKey("");
    setShowApiKey(false);
    setSelectedProviderId(next.provider && next.provider !== "opencodex" ? next.provider : (next.providers[0]?.id || ""));
  }, []);

  const load = useCallback(async () => {
    setBusy("load");
    setError(null);
    try {
        const nextReport = await managerApi.codexConfigGet();
        applyReport(nextReport);
        try {
          const nextOpenCodex = await managerApi.openCodexStatus();
          setOpenCodex(nextOpenCodex);
          setConnectionView(nextReport.activeMode === "opencodex" || nextReport.provider === "opencodex" || nextOpenCodex.enabled ? "multi" : "single");
          setRestartRequired(Boolean(nextOpenCodex.requiresCodexRestart));
      } catch { /* OpenCodex may not exist yet. */ }
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }, [applyReport]);

  const handleOpenCodexStatusChange = useCallback((next: OpenCodexStatus) => {
    setOpenCodex(next);
    if (next.requiresCodexRestart) setRestartRequired(true);
    if (next.enabled) void managerApi.codexConfigGet().then(applyReport).catch(() => undefined);
  }, [applyReport]);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => {
    if (openCodex?.requiresCodexRestart) setRestartRequired(true);
  }, [openCodex?.requiresCodexRestart]);

  useEffect(() => {
    if (contentRef.current) contentRef.current.scrollTop = 0;
  }, [module, connectionView]);

  const runReport = async (kind: string, action: () => Promise<CodexConfigReport>, success: string) => {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      const next = await action();
      applyReport(next);
      if (next.codexRunning) setRestartRequired(true);
      setNotice(success);
      return next;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setBusy(null);
    }
  };

  const activateDefaultMode = async () => {
    setBusy("mode");
    setError(null);
    setNotice(null);
    try {
      const next = await managerApi.codexConfigActivateDefault();
      applyReport(next);
      setOpenCodex((current) => current ? { ...current, enabled: false, codexProviderId: "" } : current);
      setConnectionView("single");
      setRestartRequired(next.codexRunning);
      setNotice(report?.codexRunning ? copy.savedRunning : copy.defaultModeHint);
    } catch (cause) {
      const message = errorMessage(cause);
      setError(message);
      try {
        const current = await managerApi.codexConfigGet();
        applyReport(current);
        if (current.activeMode === "opencodex" || current.provider === "opencodex") {
          setConnectionView("multi");
          setNotice("默认配置未切换，OpenCodex 多模型仍在生效。");
        }
      } catch {
        // Keep the original transition error when the follow-up status read
        // is unavailable.
      }
    } finally {
      setBusy(null);
    }
  };

  const activateMultiMode = async () => {
    if (multiActive) {
      setConnectionView("multi");
      return;
    }
    if (!openCodex?.routes.length) {
      setConnectionView("multi");
      return;
    }
    setBusy("mode");
    setError(null);
    setNotice(null);
    try {
      const next = await managerApi.openCodexActivateSaved();
      setOpenCodex(next);
      const nextReport = await managerApi.codexConfigGet();
      applyReport(nextReport);
      setConnectionView("multi");
      setRestartRequired(nextReport.codexRunning);
      setNotice(copy.multiMode + " · " + copy.available);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const activateProvider = async (profile: CodexProviderProfile) => {
    setBusy("provider-switch");
    setError(null);
    setNotice(null);
    try {
      const next = await managerApi.codexConfigSaveBasic({ ...basic, provider: profile.id, baseUrl: profile.baseUrl });
      applyReport(next);
      setSelectedProviderId(profile.id);
      setConnectionView("single");
      setRestartRequired(next.codexRunning);
      setNotice(next.codexRunning ? copy.savedRunning : copy.providerSaved);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const restartCodex = async () => {
    setBusy("restart");
    try {
      if (navigator.platform.toLowerCase().startsWith("win")) await managerApi.winRestart();
      else await managerApi.macRestart();
      setRestartRequired(false);
      setNotice(copy.savedRunning);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const checkProvider = async (profile: CodexProviderProfile) => {
    if (!profile.baseUrl) return;
    setProviderHealth((current) => ({ ...current, [profile.id]: "checking" }));
    try {
      const fetched = await managerApi.codexConfigFetchModels(profile.baseUrl);
      setProviderHealth((current) => ({ ...current, [profile.id]: fetched.length ? "connected" : "failed" }));
    } catch {
      setProviderHealth((current) => ({ ...current, [profile.id]: "failed" }));
    }
  };

  const providerProfiles = useMemo<CodexProviderProfile[]>(() => {
    if (!report) return [];
    const profiles = report.providers.filter((profile) => profile.id !== "opencodex");
    if (!profiles.some((profile) => profile.id === "osir")) {
      profiles.unshift({ id: "osir", name: "OSIR", baseUrl: "https://api.osirclaw.com/v1", wireApi: "responses" });
    }
    return profiles;
  }, [report]);

  const providerDirty = !sameBasic(basic, dialogBaseline.provider, ["model", "provider", "baseUrl"]) || Boolean(providerApiKey);
  const behaviorDirty = !sameBasic(basic, dialogBaseline.behavior, ["reasoningEffort", "personality", "approvalPolicy", "sandboxMode", "disableResponseStorage", "goalMode", "imageGenerationCompatibility"]);
  const credentialDirty = Boolean(apiKey);
  const imageDirty = Boolean(imageApiKey || imageModel !== dialogBaseline.imageModel || imageBaseUrl !== dialogBaseline.imageBaseUrl);
  const rawDirty = rawDraft !== dialogBaseline.raw;
  const mcpDirty = Boolean(draft && JSON.stringify(serverInput(draft)) !== dialogBaseline.mcp);
  const locked = Boolean(busy);
  const modelsReady = modelsBaseUrl === basic.baseUrl && models.length > 0;
  const dangerousCombination = basic.approvalPolicy === "never" && basic.sandboxMode === "danger-full-access";
  const backendMode = report?.activeMode;
  const multiActive = Boolean(backendMode === "opencodex" || report?.provider === "opencodex" || report?.openCodex?.enabled || openCodex?.enabled);
  const singleReady = Boolean((backendMode === "default" || (!backendMode && !multiActive)) && report?.model && report.apiKeyConfigured);
  const effectiveMode = backendMode === "unavailable" ? "none" : multiActive ? "multi" : singleReady ? "single" : "none";
  const activeModel = effectiveMode === "multi"
    ? openCodex?.routes.find((route) => route.locked)?.defaultModel || report?.model || ""
    : report?.model || "";
  const activeModelCapability = openCodex?.routes
    .flatMap((route) => route.modelCapabilities || [])
    .find((model) => model.modelId === activeModel);
  const reasoningEfforts = effectiveMode === "multi"
    ? activeModelCapability?.supportedReasoningEfforts || []
    : ["minimal", "low", "medium", "high", "xhigh"];
  const modeLabel = effectiveMode === "multi" ? copy.multiMode : effectiveMode === "single" ? copy.singleMode : copy.notConfigured;
  const statusTone = report?.parseError ? "error" : effectiveMode === "none" ? "neutral" : "ok";
  const statusLabel = report?.parseError ? copy.connectionError : effectiveMode === "none" ? copy.notConfigured : copy.available;
  const callPath = effectiveMode === "multi" ? "Codex → OpenCodex → Provider" : effectiveMode === "single" ? "Codex → " + (report?.provider || "Provider") : "Codex → —";
  const modules = useMemo(() => [
    { key: "connections", label: copy.connections },
    { key: "behavior", label: copy.behavior },
    { key: "tools", label: copy.tools + (report ? ` (${report.mcpServers.length})` : "") },
    { key: "advanced", label: copy.advanced },
  ], [copy, report]);

  const openProvider = (profile?: CodexProviderProfile) => {
    const next = profile ? { ...basic, provider: profile.id, baseUrl: profile.baseUrl } : { ...basic, provider: "", baseUrl: "" };
    setBasic(next);
    setDialogBaseline((current) => ({ ...current, provider: basic }));
    setProviderApiKey("");
    setProviderStep(profile ? 2 : 1);
    setModels([]);
    setModelsBaseUrl(null);
    setDialog("provider");
  };
  const openBehavior = () => {
    if (multiActive) {
      setNotice("OpenCodex 模式下，推理强度由当前模型目录决定；请在 Codex 模型选择器中调整。");
      return;
    }
    setDialogBaseline((current) => ({ ...current, behavior: basic }));
    setDialog("behavior");
  };
  const openCredential = () => { setApiKey(""); setShowApiKey(false); setDialog("credential"); };
  const openImage = () => {
    const model = report?.imageGenerationModel || "gpt-image-2";
    const baseUrl = report?.imageGenerationBaseUrl || "";
    setImageApiKey(""); setImageModel(model); setImageBaseUrl(baseUrl);
    setDialogBaseline((current) => ({ ...current, imageModel: model, imageBaseUrl: baseUrl }));
    setDialog("image");
  };
  const openRaw = () => {
    const raw = report?.raw || "";
    setRawDraft(raw); setShowSecrets(false);
    setDialogBaseline((current) => ({ ...current, raw }));
    setDialog("raw");
  };
  const openMcp = (server?: CodexMcpServer) => {
    const next = mcpDraft(server);
    setDraft(next);
    setDialogBaseline((current) => ({ ...current, mcp: JSON.stringify(serverInput(next)) }));
    setDialog("mcp");
  };
  const dirtyFor = (kind: DialogKind) => kind === "provider" ? providerDirty : kind === "behavior" ? behaviorDirty : kind === "credential" ? credentialDirty : kind === "image" ? imageDirty : kind === "mcp" ? mcpDirty : rawDirty;
  const resetDialogDraft = (kind: DialogKind) => {
    if (kind === "provider") { setBasic(dialogBaseline.provider); setProviderApiKey(""); setProviderStep(1); setModels([]); setModelsBaseUrl(null); }
    if (kind === "behavior") setBasic(dialogBaseline.behavior);
    if (kind === "credential") { setApiKey(""); setShowApiKey(false); }
    if (kind === "image") { setImageApiKey(""); setImageModel(dialogBaseline.imageModel); setImageBaseUrl(dialogBaseline.imageBaseUrl); setImageModels([]); }
    if (kind === "mcp") { const server = draft?.originalName ? report?.mcpServers.find((item) => item.name === draft.originalName) : undefined; setDraft(mcpDraft(server)); }
    if (kind === "raw") { setRawDraft(dialogBaseline.raw); setShowSecrets(false); }
  };
  const requestDialogDismiss = () => {
    if (!dialog) return;
    if (dirtyFor(dialog)) { setConfirm({ kind: "discard", returnTo: dialog }); setDialog(null); }
    else setDialog(null);
  };

  const fetchModels = async () => {
    setBusy("models"); setError(null);
    try { const fetched = await managerApi.codexConfigFetchModels(basic.baseUrl); setModels(fetched); setModelsBaseUrl(basic.baseUrl); setNotice(copy.modelsFetched.replace("{count}", String(fetched.length))); }
    catch (cause) { setError(errorMessage(cause)); } finally { setBusy(null); }
  };
  const fetchImageModels = async () => {
    setBusy("image-models"); setError(null);
    try { const fetched = await managerApi.codexConfigFetchImageModels(); setImageModels(fetched); setNotice(copy.modelsFetched.replace("{count}", String(fetched.length))); }
    catch (cause) { setError(errorMessage(cause)); } finally { setBusy(null); }
  };
  const saveProvider = async () => {
    const next = await runReport("provider", async () => { let saved = await managerApi.codexConfigSaveBasic(basic); if (providerApiKey.trim()) saved = await managerApi.codexConfigSetApiKey(providerApiKey); return saved; }, report?.codexRunning ? copy.savedRunning : copy.providerSaved);
    if (next) setDialog(null);
  };
  const saveBehavior = async () => {
    const next = await runReport("behavior", () => managerApi.codexConfigSaveBasic(basic), report?.codexRunning ? copy.savedRunning : copy.behaviorSaved);
    if (next) setDialog(null);
  };
  const saveCredential = async () => {
    const next = await runReport("api-key", () => managerApi.codexConfigSetApiKey(apiKey), copy.apiKeySaved);
    if (next) setDialog(null);
  };
  const saveImage = async () => {
    const next = await runReport("image-api-key", () => managerApi.codexConfigSetImageGenerationApiKey(imageApiKey, imageModel, imageBaseUrl), copy.imageSaved);
    if (next) setDialog(null);
  };
  const saveMcp = async () => {
    if (!draft) return;
    const next = await runReport("mcp", () => managerApi.codexConfigUpsertMcp(serverInput(draft)), copy.mcpSaved);
    if (next) { setDraft(null); setDialog(null); }
  };
  const toggleMcp = (server: CodexMcpServer, enabled: boolean) => { void runReport("mcp-toggle", () => managerApi.codexConfigUpsertMcp({ ...serverInput(mcpDraft(server)), enabled }), copy.mcpSaved); };
  const validateRaw = async () => {
    setBusy("validate"); setError(null);
    try { const result = await managerApi.codexConfigValidate(rawDraft); if (result.valid) setNotice(copy.valid); else setError(result.error || copy.invalid); }
    catch (cause) { setError(errorMessage(cause)); } finally { setBusy(null); }
  };
  const saveRaw = async () => { const next = await runReport("raw", () => managerApi.codexConfigSaveRaw(rawDraft), report?.codexRunning ? copy.savedRunning : copy.saved); if (next) setDialog(null); };
  const executeConfirm = async () => {
    if (!confirm) return;
    if (confirm.kind === "discard") { resetDialogDraft(confirm.returnTo); setConfirm(null); return; }
    if (confirm.kind === "switch-default") { setConfirm(null); await activateDefaultMode(); return; }
    if (confirm.kind === "switch-multi") { setConfirm(null); await activateMultiMode(); return; }
    if (confirm.kind === "delete-mcp") { const next = await runReport("delete-mcp", () => managerApi.codexConfigDeleteMcp(confirm.name), copy.saved); if (next) setConfirm(null); return; }
    if (confirm.kind === "delete-api-key") { const next = await runReport("delete-api-key", () => managerApi.codexConfigDeleteApiKey(), copy.apiKeyDeleted); if (next) { setConfirm(null); setDialog(null); } return; }
    if (confirm.kind === "delete-image-api-key") { const next = await runReport("delete-image-api-key", () => managerApi.codexConfigDeleteImageGenerationApiKey(), copy.saved); if (next) { setConfirm(null); setDialog(null); } return; }
    const next = await runReport("restore", () => managerApi.codexConfigRestoreBackup(), copy.restored); if (next) setConfirm(null);
  };
  const confirmText = !confirm ? { title: "", body: "", action: copy.delete, danger: true } : confirm.kind === "discard" ? { title: copy.discardTitle, body: copy.discardBody, action: copy.discard, danger: true } : confirm.kind === "switch-default" ? { title: copy.confirmDefault, body: copy.confirmDefaultBody, action: copy.enableDefault, danger: false } : confirm.kind === "switch-multi" ? { title: copy.confirmMulti, body: copy.confirmMultiBody, action: copy.enableMulti, danger: false } : confirm.kind === "delete-mcp" ? { title: copy.delete, body: copy.confirmDeleteMcp.replace("{name}", confirm.name), action: copy.delete, danger: true } : confirm.kind === "delete-api-key" ? { title: copy.deleteApiKey, body: copy.confirmDeleteApiKey, action: copy.deleteApiKey, danger: true } : confirm.kind === "delete-image-api-key" ? { title: copy.deleteImageApiKey, body: copy.confirmDeleteImageKey, action: copy.deleteImageApiKey, danger: true } : { title: copy.restore, body: copy.confirmRestore, action: copy.restore, danger: false };

  return (
    <div className="pop config-pop">
      <NavBar title={t("nav.config")} onBack={onBack} />
      <div className="config-workbench-shell">
        <div className="config-workbench-top">
        {busy === "load" && !report ? <div className="banner info" role="status"><Icon name="loader" /><span>{copy.loading}</span></div> : null}
        {error ? <div className="banner err" role="alert"><Icon name="alert" /><span>{error}</span></div> : null}
        {notice ? <div className="banner ok" role="status"><Icon name="check" /><span>{notice}</span></div> : null}
        {report ? (
          <>
            <section className="config-overview" aria-label={copy.currentMode}>
              <div className="config-overview-main">
                <div className="config-mode-mark"><Icon name={effectiveMode === "multi" ? "grid" : "sliders"} /></div>
                <div className="config-overview-copy">
                  <span className="config-eyebrow">{copy.currentMode}</span>
                  <div className="config-overview-title"><h1>{modeLabel}</h1><StatusBadge tone={statusTone} icon={statusTone === "error" ? "alert" : statusTone === "ok" ? "check" : "info"}>{statusLabel}</StatusBadge></div>
                  <span className="config-call-path"><Icon name="arrowUp" />{copy.currentPath}：{callPath}</span>
                </div>
              </div>
              <div className="config-overview-metrics">
                <SummaryItem label={copy.currentModel} value={effectiveMode === "multi" ? openCodex?.routes.find((route) => route.locked)?.defaultModel || report.model || "—" : report.model || "—"} />
                <SummaryItem label={copy.credentials} value={report.apiKeyConfigured || effectiveMode === "multi" ? copy.configured : copy.missing} />
                <SummaryItem label={copy.mcpCount} value={report.mcpServers.filter((server) => server.enabled).length} hint={"/ " + report.mcpServers.length} />
                <SummaryItem label={copy.backup} value={report.backupAvailable ? copy.backupReady : copy.noBackup} />
              </div>
            </section>
            {report.parseError ? <button className="banner err config-error-jump" type="button" onClick={() => { setModule("advanced"); openRaw(); }}><Icon name="alert" /><span>{copy.invalid}</span><Icon name="chevron" /></button> : null}
            <div className="config-module-tabs-frame"><Segmented items={modules} value={module} onChange={(next) => setModule(next as ConfigModule)} ariaLabel={t("nav.config")} className="config-module-tabs" /></div>
          </>
        ) : null}
        </div>
        <div ref={contentRef} className="scroll scroll-wide view config-workbench">
        {report ? (
          <>
            {module === "connections" ? (
              <section className="config-module-panel" aria-label={copy.connections}>
                <div className="config-mode-switch" role="group" aria-label={copy.connections}>
                  <button className={"config-mode-option" + (connectionView === "single" ? " active" : "")} type="button" aria-pressed={connectionView === "single"} onClick={() => setConnectionView("single")}>
                    <span className="config-mode-option-icon"><Icon name="sliders" /></span><span><strong>{copy.singleMode}</strong><small>{report.provider && report.provider !== "opencodex" ? report.provider + " · " + report.model : copy.notConfigured}</small></span>{effectiveMode === "single" ? <span className="config-current-dot">{copy.current}</span> : null}
                  </button>
                  <button className={"config-mode-option" + (connectionView === "multi" ? " active" : "")} type="button" aria-pressed={connectionView === "multi"} onClick={() => setConnectionView("multi")}>
                    <span className="config-mode-option-icon"><Icon name="grid" /></span><span><strong>{copy.multiMode}</strong><small>{openCodex?.installed ? (openCodex.routes.length ? "已保存 " + openCodex.modelCount + " 个模型 · 点击启用" : "OpenCodex " + (openCodex.version || "") + " · " + openCodex.modelCount + " models") : copy.notConfigured}</small></span>{effectiveMode === "multi" ? <span className="config-current-dot">{copy.current}</span> : null}
                  </button>
                </div>
                <div className="config-mode-actions" role="status">
                  {connectionView === "single" && effectiveMode !== "single" ? <button className="btn primary" type="button" disabled={Boolean(busy)} onClick={() => setConfirm({ kind: "switch-default" })}><Icon name={busy === "mode" ? "loader" : "check"} />{copy.enableDefault}</button> : null}
                  {connectionView === "multi" && effectiveMode !== "multi" && openCodex?.routes.length ? <button className="btn primary" type="button" disabled={Boolean(busy)} onClick={() => setConfirm({ kind: "switch-multi" })}><Icon name={busy === "mode" ? "loader" : "grid"} />{copy.enableMulti}</button> : null}
                  {restartRequired ? <><span className="config-restart-hint"><Icon name="info" />{copy.restartHint}</span><button className="btn ghost compact" type="button" disabled={Boolean(busy)} onClick={() => void restartCodex()}><Icon name={busy === "restart" ? "loader" : "refresh"} />{copy.restartCodex}</button></> : null}
                </div>
                {connectionView === "single" ? <SingleConnectionView report={report} providers={providerProfiles} copy={copy} singleReady={singleReady} selectedProviderId={selectedProviderId} providerHealth={providerHealth} busy={busy} onSelectProvider={setSelectedProviderId} onCheckProvider={checkProvider} onActivateProvider={activateProvider} onProvider={openProvider} onCredential={openCredential} /> : <OpenCodexPrototype onStatusChange={handleOpenCodexStatusChange} />}
              </section>
            ) : null}
            {module === "behavior" ? <BehaviorView report={report} copy={copy} onEdit={openBehavior} /> : null}
            {module === "tools" ? <ToolsView report={report} copy={copy} locked={locked} onAdd={() => openMcp()} onEdit={openMcp} onDelete={(name) => setConfirm({ kind: "delete-mcp", name })} onToggle={toggleMcp} onImage={openImage} /> : null}
            {module === "advanced" ? <AdvancedView report={report} copy={copy} onRaw={openRaw} onRestore={() => setConfirm({ kind: "restore" })} /> : null}
          </>
        ) : null}
        </div>
      </div>
      {confirm === null ? <>
        <ProviderDialog open={dialog === "provider"} copy={copy} report={report} providers={providerProfiles} basic={basic} setBasic={setBasic} step={providerStep} setStep={setProviderStep} apiKey={providerApiKey} setApiKey={setProviderApiKey} models={models} modelsReady={modelsReady} busy={busy} locked={locked} onFetch={fetchModels} onSave={saveProvider} onDismiss={requestDialogDismiss} />
        <BehaviorDialog open={dialog === "behavior"} copy={copy} basic={basic} setBasic={setBasic} reasoningEfforts={reasoningEfforts} dirty={behaviorDirty} dangerous={dangerousCombination} locked={locked} busy={busy} onSave={saveBehavior} onDismiss={requestDialogDismiss} />
        <CredentialDialog open={dialog === "credential"} copy={copy} configured={Boolean(report?.apiKeyConfigured)} apiKey={apiKey} setApiKey={setApiKey} show={showApiKey} setShow={setShowApiKey} dirty={credentialDirty} locked={locked} busy={busy} onSave={saveCredential} onDelete={() => { setDialog(null); setConfirm({ kind: "delete-api-key" }); }} onDismiss={requestDialogDismiss} />
        <ImageDialog open={dialog === "image"} copy={copy} report={report} apiKey={imageApiKey} setApiKey={setImageApiKey} model={imageModel} setModel={setImageModel} baseUrl={imageBaseUrl} setBaseUrl={setImageBaseUrl} models={imageModels} locked={locked} busy={busy} onFetch={fetchImageModels} onSave={saveImage} onDelete={() => { setDialog(null); setConfirm({ kind: "delete-image-api-key" }); }} onDismiss={requestDialogDismiss} />
        <McpDialog open={dialog === "mcp"} copy={copy} draft={draft} setDraft={setDraft} original={report?.mcpServers.find((server) => server.name === draft?.originalName)} dirty={mcpDirty} locked={locked} busy={busy} onSave={saveMcp} onDismiss={requestDialogDismiss} />
        <RawDialog open={dialog === "raw"} copy={copy} report={report} raw={rawDraft} setRaw={setRawDraft} showSecrets={showSecrets} setShowSecrets={setShowSecrets} dirty={rawDirty} locked={locked} busy={busy} onValidate={validateRaw} onSave={saveRaw} onDismiss={requestDialogDismiss} />
      </> : null}
      <Sheet open={confirm !== null} onDismiss={() => setConfirm(null)} centeredInExpanded labelledBy="config-confirm-title" initialFocus="first">
        <div className="config-confirm-dialog"><div className={"config-confirm-icon" + (confirmText.danger ? " danger" : "")}><Icon name={confirmText.danger ? "alert" : "refresh"} /></div><div><span className="config-eyebrow">{copy.advanced}</span><h2 id="config-confirm-title">{confirmText.title}</h2><p>{confirmText.body}</p></div><div className="config-dialog-actions"><button className="btn ghost" type="button" onClick={() => { if (confirm?.kind === "discard") setDialog(confirm.returnTo); if (confirm?.kind === "delete-api-key") setDialog("credential"); if (confirm?.kind === "delete-image-api-key") setDialog("image"); setConfirm(null); }}>{confirm?.kind === "discard" ? copy.keepEditing : copy.cancel}</button><button className={confirmText.danger ? "btn danger" : "btn primary"} type="button" disabled={locked} onClick={() => void executeConfirm()}><Icon name={busy ? "loader" : confirmText.danger ? "trash" : "refresh"} />{confirmText.action}</button></div></div>
      </Sheet>
    </div>
  );
}

function SingleConnectionView({ report, providers, copy, singleReady, selectedProviderId, providerHealth, busy, onSelectProvider, onCheckProvider, onActivateProvider, onProvider, onCredential }: { report: CodexConfigReport; providers: CodexProviderProfile[]; copy: Copy; singleReady: boolean; selectedProviderId: string; providerHealth: Record<string, ProviderHealth>; busy: string | null; onSelectProvider: (id: string) => void; onCheckProvider: (profile: CodexProviderProfile) => void; onActivateProvider: (profile: CodexProviderProfile) => void; onProvider: (profile?: CodexProviderProfile) => void; onCredential: () => void }) {
  const selected = providers.find((profile) => profile.id === selectedProviderId) || providers[0];
  const selectedHealth = selected ? providerHealth[selected.id] || "unknown" : "unknown";
  const active = selected?.id === report.provider;
  const healthLabel = selectedHealth === "connected" ? copy.providerConnected : selectedHealth === "failed" ? copy.providerFailed : selectedHealth === "checking" ? copy.providerChecking : copy.providerUnknown;
  const healthTone = selectedHealth === "connected" ? "ok" : selectedHealth === "failed" ? "error" : selectedHealth === "checking" ? "warn" : "neutral";
  return <div className="config-default-layout">
    <section className="config-list-card config-default-providers" aria-label={copy.providerList}>
      <header className="config-card-head"><div><span className="config-eyebrow">{copy.selectProvider}</span><h2>{providers.length} {copy.providerList}</h2><p>{copy.providerListHint}</p></div><button className="btn ghost compact" type="button" disabled={Boolean(report.parseError)} onClick={() => onProvider()}><Icon name="plus" />{copy.addProvider}</button></header>
      <div className="config-default-file"><Icon name="folder" /><div><strong className="mono">{report.path.split(/[\\/]/u).pop() || "config.toml"}</strong><span>{copy.defaultConfigFileHint}</span></div></div>
      <div className="config-resource-list">{providers.map((profile) => {
        const isActive = report.provider === profile.id;
        const isSelected = selected?.id === profile.id;
        const health = providerHealth[profile.id] || "unknown";
        return <button className={"config-resource-row config-default-provider-row" + (isSelected ? " selected" : "") + (isActive ? " active" : "")} type="button" key={profile.id} onClick={() => onSelectProvider(profile.id)}>
          <span className="config-resource-icon">{(profile.name || profile.id).slice(0, 1).toUpperCase()}</span>
          <span className="config-resource-copy"><strong>{profile.name || profile.id}</strong><span className="mono">{profile.id} · {profile.baseUrl || copy.automatic}</span></span>
          {profile.id === "osir" ? <span className="tag">{copy.recommended}</span> : null}
          {isActive ? <StatusBadge tone="ok" icon="check">{copy.current}</StatusBadge> : health === "connected" ? <StatusBadge tone="ok" icon="check">{copy.providerConnected}</StatusBadge> : health === "failed" ? <StatusBadge tone="error" icon="alert">{copy.providerFailed}</StatusBadge> : <Icon name="chevron" />}
        </button>;
      })}</div>
    </section>
    <section className="config-feature-card config-default-detail" aria-label={copy.defaultConnection}>
      <header className="config-card-head"><div><span className="config-eyebrow">{copy.defaultConnection} · {report.path.split(/[\\/]/u).pop() || "config.toml"}</span><h2>{selected?.name || selected?.id || copy.notConfigured}</h2><p>{copy.defaultModeHint}</p></div><StatusBadge tone={active && singleReady ? "ok" : healthTone} icon={active && singleReady ? "check" : selectedHealth === "failed" ? "alert" : selectedHealth === "connected" ? "check" : "info"}>{active && singleReady ? copy.current : healthLabel}</StatusBadge></header>
      <div className="config-feature-grid"><SummaryItem label={copy.currentProvider} value={selected?.name || "—"} hint={selected?.id} /><SummaryItem label={copy.currentModel} value={active ? report.model || "—" : "—"} /><SummaryItem label={copy.baseUrl} value={selected?.baseUrl || "—"} /><SummaryItem label={copy.credentials} value={active && report.apiKeyConfigured ? copy.configured : copy.missing} /></div>
      <div className="config-default-detail-note"><Icon name="info" /><span>{copy.providerListHint}</span></div>
      <div className="config-card-actions"><button className="btn ghost" type="button" disabled={!selected || Boolean(busy)} onClick={() => selected && void onCheckProvider(selected)}><Icon name={selectedHealth === "checking" ? "loader" : "refresh"} />{healthLabel}</button><button className="btn ghost" type="button" onClick={onCredential}><Icon name="shield" />{copy.rotateKey}</button><button className="btn primary" type="button" disabled={!selected || Boolean(busy) || active} onClick={() => selected && void onActivateProvider(selected)}><Icon name={busy === "provider-switch" ? "loader" : "check"} />{active ? copy.current : copy.switchProvider}</button></div>
    </section>
  </div>;
}

function BehaviorView({ report, copy, onEdit }: { report: CodexConfigReport; copy: Copy; onEdit: () => void }) {
  return <section className="config-module-panel" aria-label={copy.behavior}><section className="config-feature-card"><header className="config-card-head"><div><span className="config-eyebrow">{copy.behaviorSummary}</span><h2>{copy.behavior}</h2></div><button className="btn primary" type="button" onClick={onEdit}><Icon name="sliders" />{copy.editBehavior}</button></header><div className="config-feature-grid config-feature-grid-six"><SummaryItem label={copy.reasoning} value={report.reasoningEffort || copy.automatic} /><SummaryItem label={copy.personality} value={report.personality || copy.automatic} /><SummaryItem label={copy.goalMode} value={report.goalMode ? copy.enabled : copy.missing} /><SummaryItem label={copy.approvalPolicy} value={report.approvalPolicy || copy.automatic} /><SummaryItem label={copy.sandboxMode} value={report.sandboxMode || copy.automatic} /><SummaryItem label={copy.disableResponseStorage} value={report.disableResponseStorage ? copy.enabled : copy.missing} /></div>{report.approvalPolicy === "never" && report.sandboxMode === "danger-full-access" ? <div className="config-danger-note" role="alert"><Icon name="alert" /><span>{copy.dangerousCombination}</span></div> : null}</section></section>;
}

function ToolsView({ report, copy, locked, onAdd, onEdit, onDelete, onToggle, onImage }: { report: CodexConfigReport; copy: Copy; locked: boolean; onAdd: () => void; onEdit: (server: CodexMcpServer) => void; onDelete: (name: string) => void; onToggle: (server: CodexMcpServer, enabled: boolean) => void; onImage: () => void }) {
  return <section className="config-module-panel" aria-label={copy.tools}><div className="config-tools-grid">
    <section className="config-list-card" aria-label={copy.mcpServers}>
      <header className="config-card-head"><div><span className="config-eyebrow">{copy.mcpServers}</span><h2>{report.mcpServers.length}</h2><p>{copy.mcpHint}</p></div><button className="btn ghost compact" type="button" disabled={Boolean(report.parseError)} onClick={onAdd}><Icon name="plus" />{copy.addMcp}</button></header>
      {report.mcpServers.length ? <div className="config-resource-list">{report.mcpServers.map((server) => <div className="config-resource-row static" key={server.name}><button className="config-resource-main" type="button" onClick={() => onEdit(server)}><span className={"config-transport " + server.transport}>{server.transport}</span><span className="config-resource-copy"><strong>{server.name}</strong><span className="mono">{server.transport === "stdio" ? [server.command, ...server.args].filter(Boolean).join(" ") : server.url}</span></span>{server.hasSensitiveValues ? <span className="tag">{copy.sensitive}</span> : null}</button><Toggle checked={server.enabled} disabled={locked} ariaLabel={copy.enabled + " " + server.name} onChange={(enabled) => onToggle(server, enabled)} /><button className="btn ghost danger icon-only" type="button" aria-label={copy.delete + " " + server.name} onClick={() => onDelete(server.name)}><Icon name="trash" /></button></div>)}</div> : <div className="config-empty">{copy.emptyMcp}</div>}
    </section>
    <section className="config-feature-card config-tool-card"><header className="config-card-head"><div><span className="config-eyebrow">{copy.imageTool}</span><h2>{report.imageGenerationApiKeyConfigured ? copy.configured : copy.missing}</h2><p>{copy.imageToolHint}</p></div><StatusBadge tone={report.imageGenerationApiKeyConfigured ? "ok" : "neutral"} icon={report.imageGenerationApiKeyConfigured ? "check" : "info"}>{report.imageGenerationApiKeyConfigured ? copy.available : copy.notConfigured}</StatusBadge></header><div className="config-feature-grid"><SummaryItem label={copy.imageBaseUrl} value={report.imageGenerationBaseUrl || "—"} /><SummaryItem label={copy.imageModel} value={report.imageGenerationModel || "gpt-image-2"} /></div><div className="config-card-actions"><button className="btn primary" type="button" onClick={onImage}><Icon name="palette" />{copy.configureImage}</button></div></section>
  </div></section>;
}

function AdvancedView({ report, copy, onRaw, onRestore }: { report: CodexConfigReport; copy: Copy; onRaw: () => void; onRestore: () => void }) {
  return <section className="config-module-panel" aria-label={copy.advanced}><section className="config-feature-card"><header className="config-card-head"><div><span className="config-eyebrow">{copy.diagnostics}</span><h2>{report.parseError ? copy.connectionError : copy.valid}</h2></div><StatusBadge tone={report.parseError ? "error" : "ok"} icon={report.parseError ? "alert" : "check"}>{report.parseError ? copy.invalid : copy.valid}</StatusBadge></header><div className="config-diagnostic-list"><div><span>{copy.configFile}</span><strong className="mono" title={report.path}>{report.path}</strong><button className="btn ghost compact" type="button" onClick={() => void managerApi.openCodexHome()}><Icon name="folder" />{copy.openFolder}</button></div><div><span>{copy.parseStatus}</span><strong>{report.parseError || copy.valid}</strong></div><div><span>{copy.codexStatus}</span><strong>{report.codexRunning ? copy.running : copy.stopped}</strong></div><div><span>{copy.backup}</span><strong>{report.backupAvailable ? copy.backupReady : copy.noBackup}</strong></div></div><div className="config-card-actions"><button className="btn ghost" type="button" disabled={!report.backupAvailable} onClick={onRestore}><Icon name="refresh" />{copy.restore}</button><button className="btn primary" type="button" onClick={onRaw}><Icon name="sliders" />{copy.editRaw}</button></div></section></section>;
}

function ProviderDialog({ open, copy, report, providers, basic, setBasic, step, setStep, apiKey, setApiKey, models, modelsReady, busy, locked, onFetch, onSave, onDismiss }: { open: boolean; copy: Copy; report: CodexConfigReport | null; providers: CodexProviderProfile[]; basic: CodexBasicConfigInput; setBasic: (value: CodexBasicConfigInput) => void; step: number; setStep: (value: number) => void; apiKey: string; setApiKey: (value: string) => void; models: string[]; modelsReady: boolean; busy: string | null; locked: boolean; onFetch: () => void; onSave: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={copy.step + " " + step + " / 3"} title={step === 1 ? copy.chooseProvider : step === 2 ? copy.configureConnection : copy.chooseModel} titleId="config-provider-dialog-title" onDismiss={onDismiss} wide actions={<><button className="btn ghost" type="button" disabled={locked} onClick={step === 1 ? onDismiss : () => setStep(Math.max(1, step - 1))}>{step === 1 ? copy.cancel : copy.previous}</button>{step < 3 ? <button className="btn primary" type="button" disabled={locked || (step === 2 && (!basic.provider.trim() || !basic.baseUrl.trim()))} onClick={() => setStep(Math.min(3, step + 1))}>{copy.next}<Icon name="chevron" /></button> : <button className="btn primary" type="button" disabled={locked || !basic.provider.trim() || !basic.baseUrl.trim() || !basic.model.trim()} onClick={onSave}><Icon name={busy === "provider" ? "loader" : "check"} />{copy.saveAndEnable}</button>}</>}>
    <div className="config-step-track">{[1, 2, 3].map((item) => <span key={item} className={item <= step ? "active" : ""} />)}</div>
    {step === 1 ? <div className="config-provider-choice-grid">{providers.map((profile) => <button className={"config-provider-choice" + (basic.provider === profile.id ? " active" : "")} type="button" key={profile.id} onClick={() => setBasic({ ...basic, provider: profile.id, baseUrl: profile.baseUrl })}><span>{(profile.name || profile.id).slice(0, 1)}</span><strong>{profile.name || profile.id}</strong><small className="mono">{profile.baseUrl || copy.automatic}</small>{profile.id === "osir" ? <em>{copy.recommended}</em> : null}</button>)}<button className={"config-provider-choice" + (!providers.some((profile) => profile.id === basic.provider) ? " active" : "")} type="button" onClick={() => setBasic({ ...basic, provider: "", baseUrl: "" })}><span><Icon name="plus" /></span><strong>{copy.addProvider}</strong><small>{copy.configureConnection}</small></button></div> : null}
    {step === 2 ? <div className="config-dialog-form"><p className="config-dialog-intro">{copy.connectionHelp}</p><label className="config-field"><span>{copy.provider}</span><input className="input mono" value={basic.provider} placeholder={copy.providerPlaceholder} onChange={(event) => setBasic({ ...basic, provider: event.target.value })} /></label><label className="config-field"><span>{copy.baseUrl}</span><input className="input mono" inputMode="url" value={basic.baseUrl} placeholder="https://api.example.com/v1" onChange={(event) => setBasic({ ...basic, baseUrl: event.target.value })} /></label><label className="config-field"><span>{copy.apiKey}</span><input className="input mono" type="password" autoComplete="new-password" value={apiKey} placeholder={report?.apiKeyConfigured ? copy.configured + " · " + copy.apiKeyHint : copy.apiKeyPlaceholder} onChange={(event) => setApiKey(event.target.value)} /><small>{copy.apiKeyHint}</small></label></div> : null}
    {step === 3 ? <div className="config-dialog-form"><p className="config-dialog-intro">{copy.connectionHelp}</p><div className="config-field"><span>{copy.model}</span><div className="config-model-control">{modelsReady ? <select className="input config-select mono" aria-label={copy.model} value={basic.model} onChange={(event) => setBasic({ ...basic, model: event.target.value })}>{!models.includes(basic.model) ? <option value={basic.model}>{basic.model}</option> : null}{models.map((model) => <option key={model} value={model}>{model}</option>)}</select> : <input className="input mono" aria-label={copy.model} value={basic.model} placeholder={copy.modelPlaceholder} onChange={(event) => setBasic({ ...basic, model: event.target.value })} />}<button className="btn ghost" type="button" disabled={locked || !basic.baseUrl.trim()} onClick={onFetch}><Icon name={busy === "models" ? "loader" : "refresh"} />{copy.fetchModels}</button></div></div><div className="config-connection-preview"><span>{copy.currentPath}</span><strong className="mono">Codex → {basic.provider || "Provider"} → {basic.model || "Model"}</strong><small>{basic.baseUrl || "—"}</small></div></div> : null}
  </ConfigDialog>;
}

function BehaviorDialog({ open, copy, basic, setBasic, reasoningEfforts, dirty, dangerous, locked, busy, onSave, onDismiss }: { open: boolean; copy: Copy; basic: CodexBasicConfigInput; setBasic: (value: CodexBasicConfigInput) => void; reasoningEfforts: string[]; dirty: boolean; dangerous: boolean; locked: boolean; busy: string | null; onSave: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={copy.behaviorSummary} title={copy.editBehavior} titleId="config-behavior-dialog-title" onDismiss={onDismiss} wide actions={<><button className="btn ghost" type="button" onClick={onDismiss}>{copy.cancel}</button><button className="btn primary" type="button" disabled={locked || !dirty} onClick={onSave}><Icon name={busy === "behavior" ? "loader" : "check"} />{copy.saveBehavior}</button></>}><div className="config-dialog-form two-columns"><label className="config-field"><span>{copy.reasoning}</span><select className="input config-select" value={basic.reasoningEffort} onChange={(event) => setBasic({ ...basic, reasoningEffort: event.target.value })}><option value="">{copy.automatic}</option>{reasoningEfforts.map((effort) => <option key={effort} value={effort}>{effort}</option>)}</select></label><label className="config-field"><span>{copy.personality}</span><select className="input config-select" value={basic.personality} onChange={(event) => setBasic({ ...basic, personality: event.target.value })}><option value="">{copy.automatic}</option><option value="none">none</option><option value="friendly">friendly</option><option value="pragmatic">pragmatic</option></select></label><label className="config-field"><span>{copy.approvalPolicy}</span><select className="input config-select mono" value={basic.approvalPolicy} onChange={(event) => setBasic({ ...basic, approvalPolicy: event.target.value })}><option value="">{copy.automatic}</option><option value="untrusted">untrusted</option><option value="on-request">on-request</option><option value="never">never</option></select></label><label className="config-field"><span>{copy.sandboxMode}</span><select className="input config-select mono" value={basic.sandboxMode} onChange={(event) => setBasic({ ...basic, sandboxMode: event.target.value })}><option value="">{copy.automatic}</option><option value="read-only">read-only</option><option value="workspace-write">workspace-write</option><option value="danger-full-access">danger-full-access</option></select></label></div><div className="config-dialog-switches"><div><span>{copy.goalMode}</span><Toggle checked={basic.goalMode} ariaLabel={copy.goalMode} onChange={(goalMode) => setBasic({ ...basic, goalMode })} /></div><div><span>{copy.disableResponseStorage}</span><Toggle checked={basic.disableResponseStorage} ariaLabel={copy.disableResponseStorage} onChange={(disableResponseStorage) => setBasic({ ...basic, disableResponseStorage })} /></div><div><span><strong>{copy.imageGenerationCompatibility}</strong><small>{copy.imageGenerationCompatibilityHint}</small></span><Toggle checked={basic.imageGenerationCompatibility} ariaLabel={copy.imageGenerationCompatibility} onChange={(imageGenerationCompatibility) => setBasic({ ...basic, imageGenerationCompatibility })} /></div></div>{dangerous ? <div className="config-danger-note" role="alert"><Icon name="alert" /><span>{copy.dangerousCombination}</span></div> : null}</ConfigDialog>;
}

function CredentialDialog({ open, copy, configured, apiKey, setApiKey, show, setShow, dirty, locked, busy, onSave, onDelete, onDismiss }: { open: boolean; copy: Copy; configured: boolean; apiKey: string; setApiKey: (value: string) => void; show: boolean; setShow: (value: boolean) => void; dirty: boolean; locked: boolean; busy: string | null; onSave: () => void; onDelete: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={copy.credentials} title={copy.rotateKey} titleId="config-credential-dialog-title" onDismiss={onDismiss} actions={<><button className="btn ghost" type="button" onClick={onDismiss}>{copy.cancel}</button>{configured ? <button className="btn ghost danger" type="button" onClick={onDelete}><Icon name="trash" />{copy.deleteApiKey}</button> : null}<button className="btn primary" type="button" disabled={locked || !dirty} onClick={onSave}><Icon name={busy === "api-key" ? "loader" : "shield"} />{copy.saveApiKey}</button></>}><div className="config-dialog-form"><div className="config-credential-status"><StatusBadge tone={configured ? "ok" : "neutral"} icon={configured ? "check" : "info"}>{configured ? copy.configured : copy.missing}</StatusBadge></div><label className="config-field"><span>{copy.apiKey}</span><input className="input mono" type={show ? "text" : "password"} autoComplete="new-password" value={apiKey} placeholder={copy.apiKeyPlaceholder} onChange={(event) => setApiKey(event.target.value)} /></label><div className="config-inline-switch"><span>{copy.showApiKey}</span><Toggle checked={show} disabled={!apiKey} ariaLabel={copy.showApiKey} onChange={setShow} /></div><p className="config-dialog-intro">{copy.apiKeyHint}</p></div></ConfigDialog>;
}

function ImageDialog({ open, copy, report, apiKey, setApiKey, model, setModel, baseUrl, setBaseUrl, models, locked, busy, onFetch, onSave, onDelete, onDismiss }: { open: boolean; copy: Copy; report: CodexConfigReport | null; apiKey: string; setApiKey: (value: string) => void; model: string; setModel: (value: string) => void; baseUrl: string; setBaseUrl: (value: string) => void; models: string[]; locked: boolean; busy: string | null; onFetch: () => void; onSave: () => void; onDelete: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={copy.imageTool} title={copy.configureImage} titleId="config-image-dialog-title" onDismiss={onDismiss} wide actions={<><button className="btn ghost" type="button" onClick={onDismiss}>{copy.cancel}</button>{report?.imageGenerationApiKeyConfigured ? <button className="btn ghost danger" type="button" onClick={onDelete}><Icon name="trash" />{copy.deleteImageApiKey}</button> : null}<button className="btn primary" type="button" disabled={locked || !apiKey.trim() || !baseUrl.trim() || !model.trim()} onClick={onSave}><Icon name={busy === "image-api-key" ? "loader" : "shield"} />{copy.saveImageApiKey}</button></>}><div className="config-dialog-form"><label className="config-field"><span>{copy.imageBaseUrl}</span><input className="input mono" inputMode="url" value={baseUrl} placeholder="https://api.example.com/v1" onChange={(event) => setBaseUrl(event.target.value)} /></label><div className="config-field"><span>{copy.imageModel}</span><div className="config-model-control">{models.length ? <select className="input config-select mono" value={model} onChange={(event) => setModel(event.target.value)}>{!models.includes(model) ? <option value={model}>{model}</option> : null}{models.map((item) => <option key={item} value={item}>{item}</option>)}</select> : <input className="input mono" value={model} onChange={(event) => setModel(event.target.value)} placeholder={copy.imageModelPlaceholder} />}<button className="btn ghost" type="button" disabled={locked || !report?.imageGenerationApiKeyConfigured} onClick={onFetch}><Icon name={busy === "image-models" ? "loader" : "refresh"} />{copy.fetchImageModels}</button></div></div><label className="config-field"><span>{copy.imageApiKey}</span><input className="input mono" type="password" autoComplete="new-password" value={apiKey} placeholder={copy.imageApiKeyPlaceholder} onChange={(event) => setApiKey(event.target.value)} /><small>{copy.apiKeyHint}</small></label></div></ConfigDialog>;
}

function McpDialog({ open, copy, draft, setDraft, original, dirty, locked, busy, onSave, onDismiss }: { open: boolean; copy: Copy; draft: McpDraft | null; setDraft: (value: McpDraft | null) => void; original?: CodexMcpServer; dirty: boolean; locked: boolean; busy: string | null; onSave: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={draft?.originalName ? copy.editMcp : copy.newMcp} title={draft?.name || copy.newMcp} titleId="config-mcp-dialog-title" onDismiss={onDismiss} wide actions={<><button className="btn ghost" type="button" onClick={onDismiss}>{copy.cancel}</button><button className="btn primary" type="button" disabled={locked || !draft?.name.trim() || (draft.transport === "stdio" ? !draft.command?.trim() : !draft.url?.trim()) || !dirty} onClick={onSave}><Icon name={busy === "mcp" ? "loader" : "check"} />{copy.saveMcp}</button></>}><div className="config-dialog-form">{draft ? <><label className="config-field"><span>{copy.name}</span><input className="input mono" value={draft.name} placeholder={copy.namePlaceholder} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label><div className="config-field"><span>{copy.transport}</span><Segmented items={[{ key: "stdio", label: "stdio" }, { key: "http", label: "HTTP" }]} value={draft.transport} onChange={(value) => setDraft({ ...draft, transport: value as CodexMcpTransport })} ariaLabel={copy.transport} /></div>{draft.transport === "stdio" ? <><label className="config-field"><span>{copy.command}</span><input className="input mono" value={draft.command || ""} placeholder={copy.commandPlaceholder} onChange={(event) => setDraft({ ...draft, command: event.target.value })} /></label><label className="config-field"><span>{copy.args}</span><textarea className="input mono config-args" value={draft.argsText} onChange={(event) => setDraft({ ...draft, argsText: event.target.value })} /></label></> : <label className="config-field"><span>{copy.url}</span><input className="input mono" value={draft.url || ""} placeholder="https://example.com/mcp" onChange={(event) => setDraft({ ...draft, url: event.target.value })} /></label>}</> : null}</div>{draft ? <><div className="config-inline-switch"><span>{copy.enabled}</span><Toggle checked={draft.enabled} ariaLabel={copy.enabled} onChange={(enabled) => setDraft({ ...draft, enabled })} /></div>{draft.originalName && original?.hasSensitiveValues ? <div className="config-preserve-note"><Icon name="shield" /><span>{copy.sensitiveKept}</span></div> : null}</> : null}</ConfigDialog>;
}

function RawDialog({ open, copy, report, raw, setRaw, showSecrets, setShowSecrets, dirty, locked, busy, onValidate, onSave, onDismiss }: { open: boolean; copy: Copy; report: CodexConfigReport | null; raw: string; setRaw: (value: string) => void; showSecrets: boolean; setShowSecrets: (value: boolean) => void; dirty: boolean; locked: boolean; busy: string | null; onValidate: () => void; onSave: () => void; onDismiss: () => void }) {
  return <ConfigDialog open={open} eyebrow={copy.advanced} title={copy.editRaw} titleId="config-raw-dialog-title" onDismiss={onDismiss} wide actions={<><button className="btn ghost" type="button" onClick={onDismiss}>{copy.cancel}</button><button className="btn ghost" type="button" disabled={locked} onClick={onValidate}><Icon name={busy === "validate" ? "loader" : "check"} />{copy.validate}</button><button className="btn primary" type="button" disabled={locked || !showSecrets || !dirty} onClick={onSave}><Icon name={busy === "raw" ? "loader" : "check"} />{copy.saveRaw}</button></>}><div className="config-inline-switch"><span>{copy.showSecrets}</span><Toggle checked={showSecrets} ariaLabel={copy.showSecrets} onChange={setShowSecrets} /></div>{!showSecrets ? <div className="config-mask-note">{copy.hiddenHint}</div> : null}<textarea className="config-editor" aria-label="config.toml" value={showSecrets ? raw : report?.redactedRaw || ""} readOnly={!showSecrets} spellCheck={false} onChange={(event) => setRaw(event.target.value)} />{dirty ? <div className="config-dirty">{copy.rawDirty}</div> : null}</ConfigDialog>;
}
