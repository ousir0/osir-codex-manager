import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { errorMessage, managerApi } from "../../services/managerApi";
import type { OpenCodexConfigInput, OpenCodexStatus } from "../../shared/types";
import { Sheet } from "../Sheet";
import { Icon } from "../icons";

type ConnectionMode = "osir" | "manual" | null;

type OAuthProgress = {
  stage: string;
  state: "running" | "success" | "error";
  step: number;
  total: number;
  title: string;
  detail: string;
};

const OAUTH_STEPS = [
  { stage: "exchange", label: "读取账户与订阅" },
  { stage: "runtime", label: "准备 OpenCodex" },
  { stage: "config", label: "写入模型配置" },
  { stage: "verify", label: "验证供应商模型" },
] as const;

const ROUTES = [
  { id: "osir-gpt", label: "GPT", provider: "OSIR API", model: "gpt-5.6-sol", count: 7, accent: "blue", initials: "G" },
  { id: "osir-claude", label: "Claude", provider: "OSIR API", model: "claude-opus-5", count: 6, accent: "peach", initials: "C" },
  { id: "osir-gemini", label: "Gemini", provider: "OSIR API", model: "gemini-2.5-pro", count: 4, accent: "peach", initials: "G" },
  { id: "osir-grok", label: "Grok", provider: "OSIR API", model: "grok-4.6", count: 5, accent: "lime", initials: "X" },
] as const;

const STEPS = ["检测环境", "连接供应商", "同步模型", "完成"];

export function OpenCodexPrototype({ onStatusChange }: { onStatusChange?: (status: OpenCodexStatus) => void } = {}) {
  const [notice, setNotice] = useState("正在检测 OpenCodex 和本机配置…");
  const [connectionMode, setConnectionMode] = useState<ConnectionMode>(null);
  const [oauthSuccess, setOauthSuccess] = useState<OpenCodexStatus | null>(null);
  const [oauthError, setOauthError] = useState<string | null>(null);
  const [oauthProgress, setOauthProgress] = useState<OAuthProgress | null>(null);
  const [removeModel, setRemoveModel] = useState<{ routeId: string; model: string } | null>(null);
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

  // Users may authorize or add a provider in the OpenCodex dashboard. When
  // they return to Manager, re-read the live OpenCodex config so the new
  // provider/model list appears without restarting the Manager UI.
  useEffect(() => {
    const refreshOnFocus = () => { void refreshStatus(); };
    window.addEventListener("focus", refreshOnFocus);
    return () => window.removeEventListener("focus", refreshOnFocus);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<OAuthProgress>("opencodex://oauth-progress", (event) => {
      if (disposed) return;
      setOauthProgress(event.payload);
      if (event.payload.state === "error") setOauthError(event.payload.detail);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!status?.routes.length || status.routes.some((route) => route.id === selectedRoute)) return;
    setSelectedRoute(status.routes.find((route) => route.locked)?.id || status.routes[0].id);
  }, [selectedRoute, status]);

  useEffect(() => {
    if (status) onStatusChange?.(status);
  }, [onStatusChange, status]);

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
    setOauthError(null);
    if (!status?.installed) {
      void run("install", async () => {
        const next = await managerApi.openCodexInstall();
        setConnectionMode("osir");
        return next;
      }, "OpenCodex 已安装；下一步填写供应商凭据。");
      return;
    }
    if (status.serviceState !== "ready") {
      void run("start", async () => {
        const next = await managerApi.openCodexStart();
        setConnectionMode("osir");
        return next;
      }, "OpenCodex 已启动；请继续配置供应商。");
    }
    setConnectionMode("osir");
    setNotice("连接方式已打开；请在浏览器中完成 OSIRAPI 登录授权。");
  };

  const connectOsirOAuth = async () => {
    setBusy("oauth");
    setOauthError(null);
    setOauthProgress({ stage: "browser", state: "running", step: 0, total: 4, title: "等待浏览器授权", detail: "请在浏览器完成登录；成功后标签页会自动关闭。" });
    try {
      const next = await managerApi.openCodexConnectOsirOAuth();
      setStatus(next);
      if (next.connectionStatus !== "connected" || !next.account) {
        throw new Error(next.error || "OSIRAPI 授权已完成，但本机服务、账户或默认模型路由验证未通过。请按提示重试。");
      }
      setNotice(next.error ? "OSIRAPI 已授权并同步；默认模型遇到临时网络异常，可直接重新检测。" : "OSIRAPI 已授权，账户与模型状态已同步。");
      setConnectionMode(null);
      setOauthSuccess(next);
    } catch (cause) {
      const message = errorMessage(cause);
      setOauthError(message);
      setOauthProgress({ stage: "failed", state: "error", step: 0, total: 4, title: "连接未完成", detail: message });
    } finally {
      setBusy(null);
    }
  };

  const openManualConnection = async () => {
    setCustomEnabled(true);
    if (!status?.installed) {
      const next = await run("install", () => managerApi.openCodexInstall(), "OpenCodex 已安装；可以添加自定义供应商。");
      if (!next) return;
    } else if (status.serviceState !== "ready") {
      const next = await run("start", () => managerApi.openCodexStart(), "OpenCodex 已启动；可以添加自定义供应商。");
      if (!next) return;
    }
    setConnectionMode("manual");
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
    : ROUTES.map((route) => ({ ...route, models: [route.model], availability: "configured", locked: route.id === selectedRoute }));
  const selected = displayRoutes.find((route) => route.id === selectedRoute) ?? displayRoutes[0];
  const installed = status?.installed ?? false;
  const serviceReady = status?.serviceState === "ready";
  const osirReady = ROUTES.some((route) => Boolean(routeKeys[route.id]?.trim()));
  const customReady = customEnabled && Boolean(customRoute.id && customRoute.label && customRoute.baseUrl && customRoute.model && customRoute.apiKey);
  const canSave = osirReady || customReady;
  const stageIndex = !status || !installed ? 0 : !status.enabled ? 1 : status.modelCount === 0 ? 2 : 3;
  const connectionStatus = status?.connectionStatus || "notConnected";
  const environment = status?.environment;
  const account = status?.account;
  const activeSubscription = account?.subscriptions?.[0];
  const formatUsd = (value: number) => `$${value.toFixed(2)}`;
  const environmentLabel = environment?.runtimeState === "managed"
    ? "已发现 Manager 自带运行时"
    : environment?.runtimeState === "system"
      ? "已发现系统 OpenCodex"
      : environment?.installStrategy === "privateNpm"
        ? "将复用本机 Node/npm（安装到 Manager 私有目录）"
        : environment?.installStrategy === "managedComponent"
          ? "将下载当前平台自带运行时"
          : environment?.runtimeState === "unsupported"
            ? "当前系统或 CPU 暂无可用安装包"
            : "等待环境检测";

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
      await refreshStatus();
      setNotice(result.available ? "路由验证成功：" + selected.label + " / " + selected.model : "路由验证失败：" + result.detail);
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setRouteCheckBusy(false);
    }
  };

  const recheckOAuthDefaultRoute = async () => {
    if (!oauthSuccess) return;
    const route = oauthSuccess.routes.find((item) => item.availability === "degraded" || item.availability === "offline")
      || oauthSuccess.routes.find((item) => item.locked)
      || oauthSuccess.routes.find((item) => item.id.toLowerCase().includes("openai"))
      || oauthSuccess.routes[0];
    if (!route) {
      setNotice("模型目录中没有可验证的默认路由，请先刷新配置。");
      return;
    }
    setRouteCheckBusy(true);
    try {
      const result = await managerApi.openCodexCheckRoute(route.id, route.defaultModel);
      if (!result.available) {
        setOauthSuccess((current) => current ? { ...current, error: `模型已同步，但默认路由验证仍未通过：${result.detail}` } : current);
        setNotice(result.retryable ? "上游连接仍在波动，请稍后再次检测；无需重新授权。" : "默认模型验证失败，请检查供应商凭据或模型权限。");
        return;
      }
      const refreshed = await managerApi.openCodexStatus();
      const verified = { ...refreshed, error: null };
      setStatus(verified);
      setOauthSuccess(verified);
      setNotice(`路由验证成功：${route.label} / ${route.defaultModel}`);
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setRouteCheckBusy(false);
    }
  };

  const disconnectOsir = async () => {
    await run("disconnect", () => managerApi.openCodexDisconnectOsir(), "已退出 OSIRAPI 连接；本地历史配置仍保留。")
  };

  const removeSelectedModel = async () => {
    if (!removeModel) return;
    setBusy("remove-model");
    try {
      const next = await managerApi.openCodexRemoveModel(removeModel.routeId, removeModel.model);
      setStatus(next);
      setNotice(`已移除模型 ${removeModel.model}；模型目录已同步。`);
      setRemoveModel(null);
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const openOpenCodex = async () => {
    setBusy("console");
    try {
      let next = status;
      if (!next?.installed) next = await managerApi.openCodexInstall();
      if (next.serviceState !== "ready") next = await managerApi.openCodexStart();
      setStatus(next);
      await managerApi.openUrl(`http://127.0.0.1:${next.port || 10100}`);
      setNotice("已打开 OpenCodex 本地控制台。");
    } catch (cause) {
      setNotice(errorMessage(cause));
    } finally {
      setBusy(null);
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
            {connectionStatus === "connected" ? <div className="multi-model-connection-state connected"><span className="multi-model-state-dot" />OSIRAPI 已连接 <small>{account?.displayName || account?.email || "订阅账户"}</small></div> : <button className="btn primary multi-model-primary" type="button" disabled={Boolean(busy)} onClick={connect}><Icon name={busy === "install" ? "loader" : connectionStatus === "error" ? "refresh" : "globe"} />{!installed ? "安装多模型组件" : connectionStatus === "error" ? "重新连接" : connectionStatus === "signedOut" ? "重新登录 OSIRAPI" : "连接 OSIRAPI"}</button>}
          </div>
        </div>
        <div className="multi-model-orbit" aria-hidden="true"><div className="multi-model-orbit-ring ring-one" /><div className="multi-model-orbit-ring ring-two" /><div className="multi-model-orbit-core"><span className="multi-model-core-mark">C</span><span>CODEX</span></div><span className="multi-model-orbit-node node-gpt">G</span><span className="multi-model-orbit-node node-claude">C</span><span className="multi-model-orbit-node node-grok">X</span></div>
      </div>

      <div className="multi-model-notice" role="status"><Icon name="info" /><span>{notice}</span></div>
      {environment ? <section className="multi-model-environment" aria-label="OpenCodex 环境检测"><div><span className="multi-model-label">环境检测 · {environment.platform} / {environment.architecture}</span><strong>{environmentLabel}</strong><span>{environment.detail}</span></div><span className={environment.supported ? "multi-model-environment-badge ready" : "multi-model-environment-badge"}>{environment.supported ? "支持" : "需处理"}</span></section> : null}

      <section className={"multi-model-account-card status-" + connectionStatus} aria-label="OSIRAPI 连接状态">
        <div className="multi-model-account-status"><span className="multi-model-account-icon"><Icon name={connectionStatus === "connected" ? "check" : connectionStatus === "error" ? "alert" : "globe"} /></span><div><span className="multi-model-label">OSIRAPI 连接状态</span><strong>{connectionStatus === "connected" ? "已连接，可使用订阅模型" : connectionStatus === "error" ? "连接异常，需要重新检查" : connectionStatus === "signedOut" ? "已退出连接" : "尚未连接"}</strong><span className="multi-model-account-subline">{connectionStatus === "connected" ? "浏览器授权、订阅 Key 和本地模型目录均已同步" : connectionStatus === "error" ? "OpenCodex 或模型目录当前不可用" : connectionStatus === "signedOut" ? "本机仍保留历史配置，重新登录后可恢复" : "连接后会自动读取用户与订阅信息"}</span></div>{connectionStatus === "connected" ? <button className="btn ghost compact multi-model-account-connect" type="button" disabled={Boolean(busy)} onClick={() => setConnectionMode("osir")}><Icon name={busy ? "loader" : "sliders"} />管理连接</button> : null}</div>
        {account ? <div className="multi-model-account-details"><div><span>用户</span><strong>{account.displayName || account.email || "OSIRAPI 用户"}</strong></div><div><span>订阅套餐</span><strong>{activeSubscription?.groupName || "有效订阅"}</strong></div><div><span>月度剩余</span><strong>{activeSubscription && activeSubscription.monthlyLimitUsd > 0 ? formatUsd(activeSubscription.monthlyRemainingUsd) : "按订阅额度"}</strong></div><div><span>余额</span><strong>{formatUsd(account.balance)}</strong></div></div> : null}
        {account && activeSubscription ? <div className="multi-model-account-progress"><span>本月用量 {formatUsd(activeSubscription.monthlyUsedUsd)} / {activeSubscription.monthlyLimitUsd > 0 ? formatUsd(activeSubscription.monthlyLimitUsd) : "不限额"}</span><span>{activeSubscription.daysRemaining} 天后到期</span><div><i style={{ width: String(activeSubscription.monthlyLimitUsd > 0 ? Math.min(100, Math.max(0, activeSubscription.monthlyUsedUsd / activeSubscription.monthlyLimitUsd * 100)) : 0) + "%" }} /></div></div> : null}
        {connectionStatus === "connected" ? <div className="multi-model-account-actions"><button className="btn ghost compact" type="button" disabled={Boolean(busy)} onClick={() => void disconnectOsir()}>{busy === "disconnect" ? "退出中…" : "退出 OSIRAPI 连接"}</button></div> : null}
      </section>

      <div className="multi-model-steps" aria-label="配置进度">
        {STEPS.map((step, index) => <div className={"multi-model-step" + (index <= stageIndex ? " active" : "")} key={step}><span className="multi-model-step-index">{String(index + 1).padStart(2, "0")}</span><span>{step}</span>{index < STEPS.length - 1 ? <span className="multi-model-step-line" /> : null}</div>)}
      </div>

      <div className="multi-model-workgrid">
        <section className="multi-model-panel multi-model-connect-panel">
          <div className="multi-model-panel-head"><div><span className="multi-model-label">从这里开始</span><h2>一次连接，自动准备好</h2></div><span className="multi-model-panel-index">01</span></div>
          <div className="multi-model-methods" role="list" aria-label="模型连接方式">
            <button className="multi-model-connection-card featured" type="button" disabled={Boolean(busy)} onClick={() => { setConnectionMode("osir"); connect(); }}><span className="multi-model-connection-icon"><Icon name="globe" /></span><span className="multi-model-connection-copy"><strong>{installed ? "连接 OSIRAPI" : "安装并连接 OSIRAPI"}</strong><span>授权后自动导入多供应商模型</span></span><span className="multi-model-card-arrow">↗</span></button>
            <button className="multi-model-connection-card" type="button" disabled={Boolean(busy)} onClick={() => void openManualConnection()}><span className="multi-model-connection-icon quiet"><Icon name="plus" /></span><span className="multi-model-connection-copy"><strong>手动添加供应商</strong><span>自动准备环境后，自定义 Base URL、模型和 API Key</span></span><span className="multi-model-card-arrow">→</span></button>
          </div>
          <div className="multi-model-safe-note"><Icon name="shield" /><span>浏览器授权使用短时 PKCE 校验；长期模型 Key 不会出现在网页、链接或日志中。</span></div>
        </section>

        <section className="multi-model-panel multi-model-routes-panel">
          <div className="multi-model-panel-head"><div><span className="multi-model-label">模型路由</span><h2>已准备 {status?.modelCount || 18} 个模型</h2></div><div className="multi-model-route-head-actions"><button className="btn ghost compact" type="button" disabled={!installed || Boolean(busy)} onClick={() => void refreshStatus()}><Icon name={busy === "load" ? "loader" : "refresh"} /> 刷新配置</button><button className="btn ghost compact" type="button" disabled={!installed || Boolean(busy)} onClick={() => void run("sync", () => managerApi.openCodexSync(), "模型目录已同步；请完全退出后重新打开 Codex。")}><Icon name={busy === "sync" ? "loader" : "refresh"} /> 同步</button></div></div>
          <div className="multi-model-route-list">{displayRoutes.map((route) => { const isSelected = route.id === selectedRoute; return <button className={"multi-model-route route-" + route.accent + (isSelected ? " selected" : "")} type="button" key={route.id} onClick={() => setSelectedRoute(route.id)}><span className="multi-model-route-avatar">{route.initials}</span><span className="multi-model-route-body"><span className="multi-model-route-topline"><strong>{route.label}</strong><em>{route.provider}</em><small className={"multi-model-route-state state-" + route.availability}>{route.availability === "verified" ? "已验证" : route.availability === "degraded" ? "临时异常" : route.availability === "offline" ? "不可用" : route.availability === "configured" ? "已配置" : "待验证"}</small></span><span className="multi-model-route-model">默认 · {route.model}</span></span><span className="multi-model-route-count">{route.count}<small> 个模型</small></span><Icon name={route.locked || isSelected ? "check" : "chevron"} /></button>; })}</div>
          <div className="multi-model-model-manager"><div className="multi-model-model-manager-head"><div><span className="multi-model-label">模型管理</span><strong>{selected.label} · {selected.models.length} 个模型</strong></div><span className="multi-model-method-hint">移除后会同步 OpenCodex 和 Codex 目录</span></div><div className="multi-model-model-list">{selected.models.map((model) => <div className="multi-model-model-row" key={model}><span className="mono">{model}</span><button className="btn ghost compact danger-text" type="button" disabled={Boolean(busy)} onClick={() => setRemoveModel({ routeId: selected.id, model })}><Icon name="trash" />移除</button></div>)}</div></div>
          <div className="multi-model-default-route"><span className="multi-model-label">当前默认模型</span><strong>{selected.model}</strong><span className="multi-model-route-badge">{selected.label}</span><div className="multi-model-route-actions"><button className="btn ghost compact" type="button" disabled={Boolean(busy)} onClick={() => void selectCurrentRoute()}>{selected.locked ? "已锁定" : "锁定此路由"}</button><button className="btn ghost compact" type="button" disabled={routeCheckBusy || !installed} onClick={() => void checkCurrentRoute()}>{routeCheckBusy ? "验证中…" : "验证可用性"}</button></div></div>
        </section>
      </div>

      <section className="multi-model-runtime"><div className="multi-model-runtime-main"><div className="multi-model-runtime-mark"><span>OC</span></div><div><span className="multi-model-label">本机组件</span><strong>{installed ? "OpenCodex " + (status?.version || "已安装") + " · " + (serviceReady ? "已就绪" : "等待启动") : "尚未安装 OpenCodex"}</strong><span className="multi-model-runtime-meta"><i className={serviceReady ? "ready" : ""} />{installed ? "端口 " + (status?.port || 10100) + " · " + (status?.modelCount || 0) + " 个模型" : "安装后由 Manager 自动检测、备份并配置"}</span></div></div><div className="multi-model-runtime-actions"><button className="btn ghost compact" type="button" disabled={!installed || Boolean(busy)} onClick={() => void openOpenCodex()}><Icon name="external" />打开 OpenCodex</button><button className="multi-model-advanced-toggle" type="button" onClick={() => setAdvanced((value) => !value)}><Icon name="sliders" />{advanced ? "收起高级设置" : "高级设置"}</button></div></section>

      {advanced ? <section className="multi-model-advanced"><div><span className="multi-model-label">高级设置</span><strong>只在需要时打开</strong></div><label><span>本机端口</span><input className="input mono" value={String(status?.port || 10100)} readOnly /></label><label><span>模型目录</span><input className="input mono" value={status?.catalogPath || "~/.codex/opencodex-catalog.json"} readOnly /></label><button className="btn ghost" type="button" disabled={!status?.backupAvailable || Boolean(busy)} onClick={() => void run("restore", () => managerApi.openCodexRestore(), "已恢复上次备份；请重新检查 Codex 配置。")}><Icon name="refresh" />恢复备份</button><div className="multi-model-key-editor"><div><span className="multi-model-label">OSIR 手动配置</span><strong>填写分组 Key 后保存并同步</strong></div>{ROUTES.map((route) => <label key={route.id}><span>{route.label} API Key</span><input className="input mono" type="password" autoComplete="off" placeholder="仅保存到本机 OpenCodex 配置" value={routeKeys[route.id] || ""} onChange={(event) => setRouteKeys((current) => ({ ...current, [route.id]: event.target.value }))} /></label>)}<button className="btn ghost multi-model-add-route" type="button" onClick={() => setCustomEnabled((value) => !value)}><Icon name="plus" />{customEnabled ? "移除自定义路由" : "添加自定义路由"}</button>{customEnabled ? <div className="multi-model-custom-route"><label><span>路由 ID</span><input className="input mono" value={customRoute.id} onChange={(event) => setCustomRoute((current) => ({ ...current, id: event.target.value }))} /></label><label><span>显示名称</span><input className="input" value={customRoute.label} onChange={(event) => setCustomRoute((current) => ({ ...current, label: event.target.value }))} /></label><label><span>Base URL</span><input className="input mono" placeholder="https://provider.example/v1" value={customRoute.baseUrl} onChange={(event) => setCustomRoute((current) => ({ ...current, baseUrl: event.target.value }))} /></label><label><span>模型 ID</span><input className="input mono" placeholder="model-name" value={customRoute.model} onChange={(event) => setCustomRoute((current) => ({ ...current, model: event.target.value }))} /></label><label><span>API Key</span><input className="input mono" type="password" autoComplete="off" value={customRoute.apiKey} onChange={(event) => setCustomRoute((current) => ({ ...current, apiKey: event.target.value }))} /></label></div> : null}<button className="btn primary" type="button" disabled={!installed || !canSave || Boolean(busy)} onClick={() => void run("save", async () => { const saved = await managerApi.openCodexSave(routeInput()); await managerApi.openCodexSync(); return saved; }, "模型路由已保存并同步；请重启 Codex。")}>{busy === "save" ? "正在保存…" : "保存并同步模型"}</button></div></section> : null}
      <Sheet open={connectionMode !== null} onDismiss={() => setConnectionMode(null)} centeredAlways labelledBy="opencodex-connect-title" initialFocus="first">
        <div className="multi-model-modal">
          <div className="multi-model-modal-head"><div><span className="multi-model-label">连接方式</span><h2 id="opencodex-connect-title">{connectionMode === "manual" ? "手动添加供应商" : "连接 OSIRAPI"}</h2></div><button className="iconbtn" type="button" aria-label="关闭" onClick={() => setConnectionMode(null)}><Icon name="close" /></button></div>
          {connectionMode === "osir" ? <div className="multi-model-modal-body"><p>浏览器只负责登录。授权成功后标签页会自动关闭，Manager 将回到前台并显示本地配置进度。</p>{oauthProgress && busy === "oauth" ? <div className="multi-model-oauth-progress" role="status" aria-live="polite"><div className="multi-model-oauth-progress-head"><span className="multi-model-account-icon"><Icon name={oauthProgress.stage === "browser" ? "globe" : "loader"} /></span><div><strong>{oauthProgress.title}</strong><span>{oauthProgress.detail}</span></div></div><div className="multi-model-oauth-progress-list">{OAUTH_STEPS.map((step, index) => { const complete = oauthProgress.step > index + 1 || oauthProgress.state === "success"; const active = oauthProgress.step === index + 1 && oauthProgress.state === "running"; return <div className={(complete ? "complete" : active ? "active" : "pending")} key={step.stage}><span>{complete ? <Icon name="check" /> : active ? <Icon name="loader" /> : index + 1}</span><strong>{step.label}</strong></div>; })}</div></div> : null}{oauthError ? <div className="banner err" role="alert"><Icon name="alert" /><span>{oauthError}</span></div> : null}<button className="btn primary" type="button" disabled={Boolean(busy)} onClick={() => void connectOsirOAuth()}><Icon name={busy === "oauth" ? "loader" : oauthError ? "refresh" : "globe"} />{busy === "oauth" ? (oauthProgress?.stage === "browser" ? "等待浏览器授权…" : "正在完成本地配置…") : oauthError ? "重新授权" : "浏览器登录并连接"}</button><div className="multi-model-safe-note"><Icon name="shield" /><span>无需粘贴配置码，长期 API Key 不会显示在网页、链接或日志中。</span></div></div> : null}
          {connectionMode === "manual" ? <div className="multi-model-modal-body multi-model-manual-form"><p>填写一条自定义供应商路由。保存后会同步到 OpenCodex 和 Codex 模型目录。</p><label><span>路由 ID</span><input className="input mono" value={customRoute.id} onChange={(event) => setCustomRoute((current) => ({ ...current, id: event.target.value }))} /></label><label><span>显示名称</span><input className="input" value={customRoute.label} onChange={(event) => setCustomRoute((current) => ({ ...current, label: event.target.value }))} /></label><label><span>Base URL</span><input className="input mono" placeholder="https://provider.example/v1" value={customRoute.baseUrl} onChange={(event) => setCustomRoute((current) => ({ ...current, baseUrl: event.target.value }))} /></label><label><span>模型 ID</span><input className="input mono" placeholder="model-name" value={customRoute.model} onChange={(event) => setCustomRoute((current) => ({ ...current, model: event.target.value }))} /></label><label><span>API Key</span><input className="input mono" type="password" autoComplete="off" value={customRoute.apiKey} onChange={(event) => setCustomRoute((current) => ({ ...current, apiKey: event.target.value }))} /></label></div> : null}
          <div className="multi-model-modal-actions"><button className="btn ghost" type="button" onClick={() => setConnectionMode(null)}>{connectionMode === "manual" ? "取消" : "关闭"}</button>{connectionMode === "manual" ? <button className="btn primary" type="button" disabled={!customReady || Boolean(busy)} onClick={() => void run("save", async () => { const saved = await managerApi.openCodexSave(routeInput()); await managerApi.openCodexSync(); setConnectionMode(null); return saved; }, "供应商已保存，模型目录已同步。")}>{busy === "save" ? "保存中…" : "保存并同步"}</button> : null}</div>
        </div>
      </Sheet>

      <Sheet open={oauthSuccess !== null} onDismiss={() => setOauthSuccess(null)} centeredAlways labelledBy="opencodex-oauth-success-title" initialFocus="first">
        <div className="multi-model-modal">
          <div className="multi-model-modal-head"><div><span className="multi-model-label">连接完成</span><h2 id="opencodex-oauth-success-title">{oauthSuccess?.error ? "OSIRAPI 已授权，等待模型复检" : "OSIRAPI 已授权并同步"}</h2></div><button className="iconbtn" type="button" aria-label="关闭" onClick={() => setOauthSuccess(null)}><Icon name="close" /></button></div>
          <div className="multi-model-modal-body"><div className="multi-model-runtime-main"><div className="multi-model-runtime-mark"><Icon name={oauthSuccess?.error ? "alert" : "check"} /></div><div><strong>{oauthSuccess?.error ? "授权与模型同步已经完成，无需重新授权" : "订阅凭据已验证，可以直接使用"}</strong><span className="multi-model-runtime-meta"><i className={oauthSuccess?.error ? "" : "ready"} />OpenCodex {oauthSuccess?.serviceState === "ready" ? "已就绪" : "配置已保存"}</span></div></div>{oauthSuccess?.error ? <div className="banner err" role="alert"><Icon name="alert" /><span>{oauthSuccess.error}</span></div> : null}<div className="multi-model-safe-note"><Icon name="shield" /><span>已创建或复用 Manager 专用订阅 Key；长期 Key 不会回显。</span></div><div className="multi-model-model-list"><div className="multi-model-model-row"><span>已连接路由</span><strong>{oauthSuccess?.routes.length || 0} 条</strong></div><div className="multi-model-model-row"><span>已同步模型</span><strong>{oauthSuccess?.modelCount || 0} 个</strong></div><div className="multi-model-model-row"><span>本机服务</span><strong>127.0.0.1:{oauthSuccess?.port || 10100}</strong></div></div><p className="multi-model-method-hint">{oauthSuccess?.error ? "上游恢复后点击重新检测即可；不需要再次登录授权。" : "请完全退出并重新打开 Codex，让原生模型选择器读取最新目录。"}</p></div>
          <div className="multi-model-modal-actions"><button className="btn ghost" type="button" onClick={() => setOauthSuccess(null)}>关闭</button>{oauthSuccess?.error ? <button className="btn primary" type="button" disabled={routeCheckBusy} onClick={() => void recheckOAuthDefaultRoute()}><Icon name={routeCheckBusy ? "loader" : "refresh"} />{routeCheckBusy ? "检测中…" : "重新检测"}</button> : <button className="btn primary" type="button" onClick={() => setOauthSuccess(null)}>完成</button>}</div>
        </div>
      </Sheet>

      <Sheet open={removeModel !== null} onDismiss={() => setRemoveModel(null)} centeredInExpanded labelledBy="remove-model-title" initialFocus="dismiss">
        <div className="multi-model-modal"><div className="multi-model-modal-head"><div><span className="multi-model-label">模型管理</span><h2 id="remove-model-title">移除这个模型？</h2></div><button className="iconbtn" type="button" aria-label="关闭" onClick={() => setRemoveModel(null)}><Icon name="close" /></button></div><div className="multi-model-modal-body"><p>将从当前路由和 Codex 模型目录中移除：</p><strong className="mono">{removeModel?.model}</strong><p className="multi-model-method-hint">已有配置会保留备份；至少会保留一个可用模型。</p></div><div className="multi-model-modal-actions"><button className="btn ghost" type="button" onClick={() => setRemoveModel(null)}>取消</button><button className="btn danger" type="button" disabled={Boolean(busy)} onClick={() => void removeSelectedModel()}><Icon name="trash" />确认移除</button></div></div>
      </Sheet>
    </section>
  );
}
