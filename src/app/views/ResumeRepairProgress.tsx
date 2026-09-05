import { useEffect, useState } from "react";
import { managerApi } from "../../services/managerApi";
import { useI18n } from "../i18n";

type Progress = Awaited<ReturnType<typeof managerApi.codexResumeRepairProgress>>;

export function ResumeRepairProgress({ active }: { active: boolean }) {
  const { lang } = useI18n();
  const zh = lang.startsWith("zh");
  const [progress, setProgress] = useState<Progress | null>(null);
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const value = await managerApi.codexResumeRepairProgress();
        if (!stopped) setProgress(value);
      } catch { /* Keep the waiting explanation visible if polling fails. */ }
      if (!stopped) timer = setTimeout(() => void poll(), 750);
    };
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, [active]);
  if (!active) return null;
  const scanning = progress?.phase === "scanning" && progress.total > 0;
  const text = scanning
    ? (zh ? `正在检查旧会话 ${progress.scanned} / ${progress.total}，本轮已修复 ${progress.repaired} 个。`
      : `Checking conversations ${progress.scanned} / ${progress.total}; repaired ${progress.repaired} this pass.`)
    : (zh ? "正在等待 Codex 退出、修复会话并重新启动…" : "Waiting for Codex to exit, repair conversations, and restart…");
  return <div className="config-repair-progress" role="status" aria-live="polite">
    <strong>{text}</strong>
    {scanning ? <progress aria-label={zh ? "旧会话修复进度" : "Conversation repair progress"} max={progress.total} value={progress.scanned} /> : null}
    <p>{zh ? "首次修复大量历史可能需要数分钟。请保持管理器打开，等待 Codex 自动启动，期间请勿手动打开 Codex。" : "The first repair of a large history may take several minutes. Keep Manager open and wait for Codex to restart automatically; do not open it manually during repair."}</p>
  </div>;
}
