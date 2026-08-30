//! Small renderer-only layout repairs for the stock Codex UI.
//!
//! This module deliberately does not alter Codex's bundled files or model
//! catalog. It only keeps the native model picker rows from shrinking inside
//! the fixed-height menu used by the current Codex renderer.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::watch;

use crate::cdp::{is_theme_excluded_target, list_app_targets, probe_session, CdpSession};

const TICK: Duration = Duration::from_millis(900);
/// The model picker is the only stock menu with the w-[280px] content width
/// and cmdk-item/menuitem rows in the current Codex renderer. The selector is
/// intentionally narrow so reasoning, permissions, and other menus keep
/// their native layout.
pub const MODEL_PICKER_LAYOUT_FIX_EXPRESSION: &str = r##"(() => {
  const id = 'codex-manager-model-picker-layout-fix';
  const css = [
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([cmdk-item]),',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([cmdk-item]),',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([role="menuitem"]),',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([role="menuitem"]),',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([role="option"]),',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([role="option"]) {',
    '  height: fit-content !important;',
    '  max-height: min(66.666vh, 640px) !important;',
    '  overflow-y: auto !important;',
    '}',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([cmdk-item]) [cmdk-item],',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([cmdk-item]) [cmdk-item],',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([role="menuitem"]) [role="menuitem"],',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([role="menuitem"]) [role="menuitem"],',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class~="w-[280px]"]:has([role="option"]) [role="option"],',
    '[data-codex-window-type="electron"] body:has([data-codex-intelligence-trigger]) [class*="w-[280px]"]:has([role="option"]) [role="option"] {',
    '  flex: 0 0 auto !important;',
    '  flex-shrink: 0 !important;',
    '  min-height: var(--menu-item-height, 36px) !important;',
    '}',
  ].join('\n');
  let style = document.getElementById(id);
  if (!(style instanceof HTMLStyleElement)) {
    style?.remove();
    style = document.createElement('style');
    style.id = id;
    document.head?.appendChild(style);
  }
  if (style.textContent !== css) style.textContent = css;
  return Boolean(style.isConnected);
})()"##;

/// Remove the repair from a renderer. This is used when OpenCodex mode is
/// disabled or the manager no longer owns the Codex launch path.
pub const REMOVE_MODEL_PICKER_LAYOUT_FIX_EXPRESSION: &str = r##"(() => {
  document.getElementById('codex-manager-model-picker-layout-fix')?.remove();
  return true;
})()"##;

/// Idempotent presence probe used to recover after a renderer reload.
pub const MODEL_PICKER_LAYOUT_FIX_PRESENT_EXPRESSION: &str =
    "Boolean(document.getElementById('codex-manager-model-picker-layout-fix'))";

/// Apply the repair immediately to the currently connected Codex renderers.
/// The background daemon still owns recovery after reloads and new windows.
pub async fn apply_model_picker_layout_fix_once(port: u16, timeout: Duration) -> crate::Result<usize> {
    let targets = crate::cdp::connect_codex_targets(port, timeout).await?;
    let mut applied = 0;
    for target in targets {
        if target
            .session
            .evaluate(MODEL_PICKER_LAYOUT_FIX_EXPRESSION)
            .await
            .is_ok()
        {
            applied += 1;
        }
        target.session.close();
    }
    Ok(applied)
}

/// Keep the renderer-only repair alive across reloads and newly opened Codex
/// windows. The daemon is enabled only for the OpenCodex launch path.
pub async fn run_model_picker_layout_daemon(port: u16, mut enabled_rx: watch::Receiver<bool>) {
    let mut sessions: HashMap<String, CdpSession> = HashMap::new();
    let mut enabled = *enabled_rx.borrow();

    loop {
        if enabled {
            match list_app_targets(port).await {
                Ok(targets) => {
                    let active: std::collections::HashSet<&str> =
                        targets.iter().map(|target| target.id.as_str()).collect();
                    sessions.retain(|id, session| {
                        let keep = active.contains(id.as_str()) && !session.closed();
                        if !keep {
                            session.close();
                        }
                        keep
                    });

                    for target in targets {
                        if is_theme_excluded_target(&target) || sessions.contains_key(&target.id) {
                            continue;
                        }
                        let id = target.id.clone();
                        match CdpSession::connect(target, port).await {
                            Ok(session) => match probe_session(&session).await {
                                Ok(probe) if probe.codex => {
                                    sessions.insert(id, session);
                                }
                                Ok(_) | Err(_) => session.close(),
                            },
                            Err(_) => {}
                        }
                    }
                }
                Err(_) => {
                    for session in sessions.values() {
                        session.close();
                    }
                    sessions.clear();
                }
            }

            let mut dead = Vec::new();
            for (id, session) in &sessions {
                let present = session
                    .evaluate(MODEL_PICKER_LAYOUT_FIX_PRESENT_EXPRESSION)
                    .await
                    .ok()
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                if !present
                    && session
                        .evaluate(MODEL_PICKER_LAYOUT_FIX_EXPRESSION)
                        .await
                        .is_err()
                {
                    dead.push(id.clone());
                }
            }
            for id in dead {
                if let Some(session) = sessions.remove(&id) {
                    session.close();
                }
            }
        } else {
            for session in sessions.values() {
                let _ = session
                    .evaluate(REMOVE_MODEL_PICKER_LAYOUT_FIX_EXPRESSION)
                    .await;
                session.close();
            }
            sessions.clear();
        }

        tokio::select! {
            changed = enabled_rx.changed() => {
                if changed.is_err() {
                    for session in sessions.values() {
                        session.close();
                    }
                    return;
                }
                enabled = *enabled_rx.borrow();
            }
            _ = tokio::time::sleep(TICK) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MODEL_PICKER_LAYOUT_FIX_EXPRESSION;

    #[test]
    fn fix_is_scoped_to_the_native_model_picker() {
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("w-[280px]"));
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("data-codex-intelligence-trigger"));
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("flex-shrink: 0"));
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("height: fit-content"));
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("66.666vh"));
        assert!(MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("var(--menu-item-height, 36px)"));
        assert!(!MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("width: min(360px"));
        assert!(!MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("display: flex"));
        assert!(
            MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("codex-manager-model-picker-layout-fix")
        );
        assert!(!MODEL_PICKER_LAYOUT_FIX_EXPRESSION.contains("[role=menuitem] {"));
    }
}
