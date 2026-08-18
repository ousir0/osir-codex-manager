import { useEffect, useState } from "react";

import { errorMessage, managerApi } from "../../services/managerApi";
import type { OpenCodexConfigInput, OpenCodexStatus } from "../../shared/types";
import { Icon } from "../icons";

type SetupState = "ready" | "connecting";

const ROUTES = [
  { id: "osir-gpt", label: "GPT", provider: "OSIR API", model: "gpt-5.6-sol", count: 7, accent: "blue", initials: "G" },
  { id: "osir-claude", label: "Claude", provider: "OSIR API", model: "claude-opus-5", count: 6, accent: "peach", initials: "C" },
  { id: "osir-grok", label: "Grok", provider: "OSIR API", model: "grok-4.6", count: 5, accent: "lime", initials: "X" },
] as const;

const STEPS = ["检测环境", "连接供应商", "同步模型", "完成"];

export function OpenCodexPrototype() {
  const [setupState, setSetupState] = useState<SetupState>("ready");
  const [notice, setNotice] = useState("正在检测 OpenCodex 和本机配置…");
  const [codeMode, setCodeMode] = useState(false);
  const [connectionCode, setConnectionCode] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [selectedRoute, setSelectedRoute] = useState("osir-gpt");
  const [status, setStatus] = useState<OpenCodexStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [routeKeys, setRouteKeys] = useState<Record<string, string>>({});
  const [routeCheckBusy, setRouteCheckBusy] = useState(false);
  const [customEnabled, setCustomEnabled] = useState(false);
  const [customRoute, setCustomRoute] = useState({
    id: "custom-provider",
    label: "自定义供应商",
    baseUrl: "",
    model: "",
    apiKey: "",
  });

  const refreshStatus = async () => {
    try {
      const next = await managerApi.openCodexStatus();
      setStatus(next);
      setNotice(next.installed ? "已检测到 OpenCodex；可以继续连接供应商或管理模型。" : "尚未安装 OpenCodex；点击主按钮即可开始安装。");
      return next;
    } catch (cause) {
      setNotice(errorMessage(cause));
      return null;
    }
  };

  useEffect(() => {
    void refreshStatus();
  }, []);

  const run = async (kind: string, action: () => Promise<OpenCodexStatus>, success: string) => {
    setBusy(kind);
    try {
      const next = await action();
      setStatus(next);
      setNotice(success);
      return next;
    } catch (cause) {
      setNotice(errorMessage(cause));
      return null;
    } finally {
      setBusy(null);
    }
  };

  const routeInput = (): OpenCodexConfigInput => {
    const routes: OpenCodexConfigInput["routes"] = ROUTES.map((route) => ({
      id: route.id,
      label: route.label,
      adapter: "openai-responses",
      baseUrl: "https://api.osirclaw.com/v1",
      apiKey: routeKeys[route.id] || undefined,
      models: [route.model],
      defaultModel: route.model,
      enabled: Boolean(routeKeys[route.id]?.trim()),
    }));
    if (customEnabled) {
      routes.push({
        id: customRoute.id,
        label: customRoute.label,
        adapter: "openai-responses",
        baseUrl: customRoute.baseUrl,
        apiKey: customRoute.apiKey || undefined,
        models: [customRoute.model],
        defaultModel: customRoute.model,
        enabled: true,
      });
    }
    const osirReady = ROUTES.every((route) => Boolean(routeKeys[route.id]?.trim()));
    return {
      enabled: true,
      port: status?.port || 10100,
      codexProviderId: status?.codexProviderId || "opencodex",
      defaultRoute: osirReady ? "osir-gpt/gpt-5.6-sol" : customRoute.id + "/" + customRoute.model,
      routes,
    };
  };

  const connect = () => {
    if (!status?.installed) {
      void run("install", () => managerApi.openCodexInstall(), "OpenCodex 已安装；下一步填写供应商凭据。");
      return;
    }
    if (status.serviceState !== "ready") {
      void run("start", () => managerApi.openCodexStart(), "OpenCodex 已启动；请继续粘贴 OSIRAPI 连接码。");
    }
    setSetupState("connecting");
    setCodeMode(true);
    setNotice("请从 OSIRAPI 订阅页复制一次性连接码，粘贴后由 Manager 自动完成加密配置和模型同步。");
    window.setTimeout(() => setSetupState("ready"), 500);
  };

  const displayRoutes = status?.routes.length
    ? status.routes.map((route, index) => ({
      ...route,
      provider: route.label,
      model: route.defaultModel,
      count: route.models.length,
      initials: index === 0 ? "G" : index === 1 ? "C" : "X",
      accent: index === 0 ? "blue" : index === 1 ? "peach" : "lime",
    }))
    : ROUTES.map((route) => ({ ...route, availability: "configured", locked: route.id === selectedRoute }));
  const selected = displayRoutes.find((route) => route.id === selectedRoute) ?? displayRoutes[0];
  const installed = status?.installed ?? false;
  const serviceReady = status?.serviceState === "ready";
  const osirReady = ROUTES.every((route) => Boolean(routeKeys[route.id]?.trim()));
  const customReady = customEnabled && Boolean(customRoute.id && customRoute.label && customRoute.baseUrl && customRoute.model && customRoute.apiKey);
  const canSave = osirReady || customReady;
  const stageIndex = !status || !installed ? 0 : !status.enabled ? 1 : status.modelCount === 0 ? 2 : 3;

  const selectCurrentRoute = async () => {
    if (!status?.enabled) {
      setSelectedRoute(selected.id);
      setNotice("预览：启用 OpenCodex 后，这里会锁定默认路由并关闭自动切换。");
      return;
    }
    setBusy("select");
    try {
      const next = await managerApi.openCodexSelectRoute(selected.id, selected.model);
      setStatus(next);
      setNotice("已锁定 " + selected.label + " / " + selected.model + "；不会自动切换到其他路由。");
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const checkCurrentRoute = async () => {
    setRouteCheckBusy(true);
    try {
      const result = await managerApi.openCodexCheckRoute(selected.id, selected.model);
      setNotice(result.available ? "路由验证成功：" + selected.label + " / " + selected.model : "路由验证失败：" + result.detail);
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setRouteCheckBusy(false);
    }
  };

  return (
    <section className="multi-model-prototype" aria-label="OpenCodex 多模型原型">
      <div className="multi-model-hero">
        <div className="multi-model-hero-copy">
          <div className="multi-model-kicker"><span className="multi-model-kicker-dot" />OPENCODEX / MULTI-MODEL<span className="multi-model-prototype-chip">可操作预览</span></div>
          <h1>把所有模型，装进 Codex 选择器。</h1>
          <p>用一个简单的连接流程，把 GPT、Claude、Grok 和其他供应商的模型放进 Codex 的原生选择器。</p>
          <div className="multi-model-hero-actions">
            <button className="btn primary multi-model-primary" type="button" disabled={Boolean(busy)} onClick={connect}><Icon name={busy === "install" || setupState === "connecting" ? "loader" : "globe"} />{!installed ? "安装多模型组件" : setupState === "connecting" ? "正在准备连接…" : "连接 OSIRAPI"}</button>
            <button className="btn ghost multi-model-secondary" type="button" disabled={Boolean(busy)} onClick={() => setCodeMode((value) => !value)}><Icon name="copy" />{codeMode ? "收起配置码" : "粘贴配置码"}</button>
          </div>
          {codeMode ? <div className="multi-model-codebox"><input className="input mono" value={connectionCode} onChange={(event) => setConnectionCode(event.target.value)} placeholder="粘贴一次性 OSIRAPI 连接码，不包含 API Key" aria-label="连接码" /><button className="btn ghost" type="button" disabled={!connectionCode.trim() || Boolean(busy)} onClick={() => void run("connect", () => managerApi.openCodexConnectOsir(connectionCode), "OSIRAPI 已连接，模型目录已同步；请完全退出后重新打开 Codex。")}>{busy === "connect" ? "正在连接…" : "校验并导入"}</button></div> : null}
        </div>
        <div className="multi-model-orbit" aria-hidden="true"><div className="multi-model-orbit-ring ring-one" /><div className="multi-model-orbit-ring ring-two" /><div className="multi-model-orbit-core"><span className="multi-model-core-mark">C</span><span>CODEX</span></div><span className="multi-model-orbit-node node-gpt">G</span><span className="multi-model-orbit-node node-claude">C</span><span className="multi-model-orbit-node node-grok">X</span></div>
      </div>

      <div className="multi-model-notice" role="status"><Icon name="info" /><span>{notice}</span></div>

      <div className="multi-model-steps" aria-label="配置进度">
        {STEPS.map((step, index) => <div className={"multi-model-step" + (index <= stageIndex ? " active" : "")} key={step}><span className="multi-model-step-index">{String(index + 1).padStart(2, "0")}</span><span>{step}</span>{index < STEPS.length - 1 ? <span className="multi-model-step-line" /> : null}</div>)}
      </div>

      <div className="multi-model-workgrid">
        <section className="multi-model-panel multi-model-connect-panel">
          <div className="multi-model-panel-head"><div><span className="multi-model-label">从这里开始</span><h2>一次连接，自动准备好</h2></div><span className="multi-model-panel-index">01</span></div>
          <button className="multi-model-connection-card featured" type="button" disabled={Boolean(busy)} onClick={connect}><span className="multi-model-connection-icon"><Icon name="globe" /></span><span className="multi-model-connection-copy"><strong>{installed ? "连接 OSIRAPI" : "安装并连接 OSIRAPI"}</strong><span>登录授权后，自动配置 GPT / Claude / Grok</span></span><span className="multi-model-card-arrow">↗</span></button>
          <button className="multi-model-connection-card" type="button" onClick={() => setCodeMode(true)}><span className="multi-model-connection-icon quiet"><Icon name="copy" /></span><span className="multi-model-connection-copy"><strong>粘贴配置码</strong><span>适合团队分发或其他供应商渠道</span></span><span className="multi-model-card-arrow">→</span></button>
          <button className="multi-model-connection-card" type="button" onClick={() => setAdvanced(true)}><span className="multi-model-connection-icon quiet"><Icon name="plus" /></span><span className="multi-model-connection-copy"><strong>手动添加供应商</strong><span>仅在高级设置中配置 Base URL 和 API Key</span></span><span className="multi-model-card-arrow">→</span></button>
          <div className="multi-model-safe-note"><Icon name="shield" /><span>连接码短时有效，不包含长期 API Key；已有 Codex 配置会先自动备份。</span></div>
        </section>

        <section className="multi-model-panel multi-model-routes-panel">
          <div className="multi-model-panel-head"><div><span className="multi-model-label">模型路由</span><h2>已准备 {status?.modelCount || 18} 个模型</h2></div><button className="btn ghost compact" type="button" disabled={!installed || Boolean(busy)} onClick={() => void run("sync", () => managerApi.openCodexSync(), "模型目录已同步；请完全退出后重新打开 Codex。")}><Icon name={busy === "sync" ? "loader" : "refresh"} /> 同步</button></div>
          <div className="multi-model-route-list">{displayRoutes.map((route) => { const isSelected = route.id === selectedRoute; return <button className={"multi-model-route route-" + route.accent + (isSelected ? " selected" : "")} type="button" key={route.id} onClick={() => setSelectedRoute(route.id)}><span className="multi-model-route-avatar">{route.initials}</span><span className="multi-model-route-body"><span className="multi-model-route-topline"><strong>{route.label}</strong><em>{route.provider}</em><small className={"multi-model-route-state state-" + route.availability}>{route.availability === "verified" ? "已验证" : route.availability === "offline" ? "不可用" : route.availability === "configured" ? "已配置" : "待验证"}</small></span><span className="multi-model-route-model">默认 · {route.model}</span></span><span className="multi-model-route-count">{route.count}<small> 个模型</small></span><Icon name={route.locked || isSelected ? "check" : "chevron"} /></button>; })}</div>
          <div className="multi-model-default-route"><span className="multi-model-label">当前默认模型</span><strong>{selected.model}</strong><span className="multi-model-route-badge">{selected.label}</span><div className="multi-model-route-actions"><button className="btn ghost compact" type="button" disabled={Boolean(busy)} onClick={() => void selectCurrentRoute()}>{selected.locked ? "已锁定" : "锁定此路由"}</button><button className="btn ghost compact" type="button" disabled={routeCheckBusy || !installed} onClick={() => void checkCurrentRoute()}>{routeCheckBusy ? "验证中…" : "验证可用性"}</button></div></div>
        </section>
      </div>

      <section className="multi-model-runtime"><div className="multi-model-runtime-main"><div className="multi-model-runtime-mark"><span>OC</span></div><div><span className="multi-model-label">本机组件</span><strong>{installed ? "OpenCodex " + (status?.version || "已安装") + " · " + (serviceReady ? "已就绪" : "等待启动") : "尚未安装 OpenCodex"}</strong><span className="multi-model-runtime-meta"><i className={serviceReady ? "ready" : ""} />{installed ? "端口 " + (status?.port || 10100) + " · " + (status?.modelCount || 0) + " 个模型" : "安装后由 Manager 自动检测、备份并配置"}</span></div></div><button className="multi-model-advanced-toggle" type="button" onClick={() => setAdvanced((value) => !value)}><Icon name="sliders" />{advanced ? "收起高级设置" : "高级设置"}</button></section>

      {advanced ? <section className="multi-model-advanced"><div><span className="multi-model-label">高级设置</span><strong>只在需要时打开</strong></div><label><span>本机端口</span><input className="input mono" value={String(status?.port || 10100)} readOnly /></label><label><span>模型目录</span><input className="input mono" value={status?.catalogPath || "~/.codex/opencodex-catalog.json"} readOnly /></label><button className="btn ghost" type="button" disabled={!status?.backupAvailable || Boolean(busy)} onClick={() => void run("restore", () => managerApi.openCodexRestore(), "已恢复上次备份；请重新检查 Codex 配置。")}><Icon name="refresh" />恢复备份</button><div className="multi-model-key-editor"><div><span className="multi-model-label">OSIR 手动配置</span><strong>填写分组 Key 后保存并同步</strong></div>{ROUTES.map((route) => <label key={route.id}><span>{route.label} API Key</span><input className="input mono" type="password" autoComplete="off" placeholder="仅保存到本机 OpenCodex 配置" value={routeKeys[route.id] || ""} onChange={(event) => setRouteKeys((current) => ({ ...current, [route.id]: event.target.value }))} /></label>)}<button className="btn ghost multi-model-add-route" type="button" onClick={() => setCustomEnabled((value) => !value)}><Icon name="plus" />{customEnabled ? "移除自定义路由" : "添加自定义路由"}</button>{customEnabled ? <div className="multi-model-custom-route"><label><span>路由 ID</span><input className="input mono" value={customRoute.id} onChange={(event) => setCustomRoute((current) => ({ ...current, id: event.target.value }))} /></label><label><span>显示名称</span><input className="input" value={customRoute.label} onChange={(event) => setCustomRoute((current) => ({ ...current, label: event.target.value }))} /></label><label><span>Base URL</span><input className="input mono" placeholder="https://provider.example/v1" value={customRoute.baseUrl} onChange={(event) => setCustomRoute((current) => ({ ...current, baseUrl: event.target.value }))} /></label><label><span>模型 ID</span><input className="input mono" placeholder="model-name" value={customRoute.model} onChange={(event) => setCustomRoute((current) => ({ ...current, model: event.target.value }))} /></label><label><span>API Key</span><input className="input mono" type="password" autoComplete="off" value={customRoute.apiKey} onChange={(event) => setCustomRoute((current) => ({ ...current, apiKey: event.target.value }))} /></label></div> : null}<button className="btn primary" type="button" disabled={!installed || !canSave || Boolean(busy)} onClick={() => void run("save", async () => { const saved = await managerApi.openCodexSave(routeInput()); await managerApi.openCodexSync(); return saved; }, "模型路由已保存并同步；请重启 Codex。")}>{busy === "save" ? "正在保存…" : "保存并同步模型"}</button></div></section> : null}
    </section>
  );
}
