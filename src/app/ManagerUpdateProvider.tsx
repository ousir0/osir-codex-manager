import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

import { managerApi, SETTINGS_CHANGED_EVENT, type ManagerUpdateAvailable } from "../services/managerApi";
import { DEFAULT_SETTINGS, type AppSettings } from "../shared/types";
import { Sheet } from "./Sheet";
import { Ring } from "./components";
import { useI18n } from "./i18n";

const DISMISSED_KEY = "cam.manager.update.dismissed";
const SKIPPED_KEY = "cam.manager.update.skipped";
const PENDING_KEY = "cam.manager.update.pending";
const APP_VERSION = import.meta.env.VITE_APP_VERSION ?? "0.0.0";

interface PendingInstall {
  version: string;
  startedAt: number;
}

interface ManagerUpdateContextValue {
  available: ManagerUpdateAvailable | null;
  check: () => Promise<void>;
}

const ManagerUpdateContext = createContext<ManagerUpdateContextValue | null>(null);

function storageValue(storage: Storage, key: string): string | null {
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

function setStorageValue(storage: Storage, key: string, value: string | null) {
  try {
    if (value === null) storage.removeItem(key);
    else storage.setItem(key, value);
  } catch {
    // Storage can be unavailable in hardened browser previews. The update
    // flow remains usable for the current session.
  }
}

function versionParts(value: string): number[] {
  return value
    .replace(/^v/i, "")
    .split(/[+-]/u, 1)[0]
    .split(".")
    .map((part) => Number.parseInt(part, 10))
    .map((part) => (Number.isFinite(part) ? part : 0));
}

function versionAtLeast(current: string, target: string): boolean {
  const left = versionParts(current);
  const right = versionParts(target);
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const a = left[index] ?? 0;
    const b = right[index] ?? 0;
    if (a !== b) return a > b;
  }
  return true;
}

function readPendingInstall(): PendingInstall | null {
  const raw = storageValue(window.localStorage, PENDING_KEY);
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<PendingInstall>;
    if (typeof value.version !== "string" || !Number.isFinite(value.startedAt)) return null;
    return { version: value.version, startedAt: Number(value.startedAt) };
  } catch {
    return null;
  }
}

function shouldSuppress(version: string): boolean {
  return storageValue(window.localStorage, SKIPPED_KEY) === version ||
    storageValue(window.sessionStorage, DISMISSED_KEY) === version;
}

export function ManagerUpdateProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [available, setAvailable] = useState<ManagerUpdateAvailable | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const checkingRef = useRef(false);

  const check = useCallback(async () => {
    if (checkingRef.current) return;
    checkingRef.current = true;
    try {
      const pending = readPendingInstall();
      if (pending && versionAtLeast(APP_VERSION, pending.version)) {
        setStorageValue(window.localStorage, PENDING_KEY, null);
        setMessage(t("about.mgrUpToDate"));
      }
      const result = await managerApi.checkManagerUpdate();
      if (result.kind !== "available") {
        return;
      }
      if (!shouldSuppress(result.version)) setAvailable(result);
    } finally {
      checkingRef.current = false;
    }
  }, [t]);

  useEffect(() => {
    let active = true;
    void managerApi.getSettings().then((next) => {
      if (active) setSettings(next);
    }).catch(() => undefined);
    return () => { active = false; };
  }, []);

  useEffect(() => {
    const onSettingsChanged = (event: Event) => {
      const next = (event as CustomEvent<AppSettings>).detail;
      if (next) setSettings(next);
    };
    window.addEventListener(SETTINGS_CHANGED_EVENT, onSettingsChanged);
    return () => window.removeEventListener(SETTINGS_CHANGED_EVENT, onSettingsChanged);
  }, []);

  useEffect(() => {
    if (!settings.checkOnStartup) return;
    const timer = window.setTimeout(() => { void check(); }, 900);
    return () => window.clearTimeout(timer);
  }, [check, settings.checkOnStartup]);

  useEffect(() => {
    if (!readPendingInstall()) return;
    void check();
  }, [check]);

  useEffect(() => {
    if (!settings.periodicCheck) return;
    const interval = window.setInterval(() => { void check(); }, Math.max(60_000, settings.periodicCheckIntervalSeconds * 1000));
    return () => window.clearInterval(interval);
  }, [check, settings.periodicCheck, settings.periodicCheckIntervalSeconds]);

  const dismiss = () => {
    if (!available) return;
    setStorageValue(window.sessionStorage, DISMISSED_KEY, available.version);
    void available.discard();
    setAvailable(null);
  };

  const skip = () => {
    if (!available) return;
    setStorageValue(window.localStorage, SKIPPED_KEY, available.version);
    void available.discard();
    setAvailable(null);
  };

  const install = async () => {
    if (!available || busy) return;
    setBusy(true);
    setStorageValue(window.localStorage, PENDING_KEY, JSON.stringify({ version: available.version, startedAt: Date.now() } satisfies PendingInstall));
    try {
      await available.installAndRelaunch();
      setAvailable(null);
      setBusy(false);
    } catch (cause) {
      setStorageValue(window.localStorage, PENDING_KEY, null);
      setMessage(cause instanceof Error ? cause.message : String(cause));
      setBusy(false);
    }
  };

  return (
    <ManagerUpdateContext.Provider value={{ available, check }}>
      {children}
      {message ? <div className="manager-update-toast" role="status"><span>{message}</span><button type="button" onClick={() => setMessage(null)} aria-label={t("confirm.cancel")}>×</button></div> : null}
      <Sheet open={Boolean(available)} onDismiss={busy ? undefined : dismiss} dismissable={!busy} centeredAlways labelledBy="manager-update-title" describedBy="manager-update-body" initialFocus="primary">
        <Ring icon="arrowUp" />
        <h3 id="manager-update-title">{available ? t("confirm.title", { version: available.version }) : ""}</h3>
        <p id="manager-update-body">{available?.body || t("about.mgrConfirmBody")}</p>
        <p className="manager-update-version">{t("about.version", { v: APP_VERSION })} → {available?.version}</p>
        <div className="row2 sheet-actions">
          <button className="btn ghost" type="button" onClick={dismiss} disabled={busy}>{t("confirm.cancel")}</button>
          <button className="btn ghost" type="button" onClick={skip} disabled={busy}>{t("home.skipCurrent")}</button>
          <button className="btn primary" type="button" onClick={() => void install()} disabled={busy}>{busy ? t("progress.installing") : t("confirm.ok")}</button>
        </div>
      </Sheet>
    </ManagerUpdateContext.Provider>
  );
}

export function useManagerUpdate() {
  const context = useContext(ManagerUpdateContext);
  if (!context) throw new Error("useManagerUpdate must be used within ManagerUpdateProvider");
  return context;
}
