//! Operation phase model and quit/shutdown policy.
//!
//! Long-running install/update work progresses through named phases. Safe
//! phases remain interruptible (pause/cancel/quit-after-cancel). Once the
//! first destructive rename begins (`Committing`) the op is at the point of
//! no return and process exit is blocked so the swap can finish or crash
//! recovery can take over on the next launch.

use serde::{Deserialize, Serialize};

use crate::app::oplock::OperationKind;

/// Lifecycle phase of the currently held operation lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OperationPhase {
    /// No active operation (or phase not yet claimed).
    #[default]
    Idle,
    /// Planning / preflight / cache setup. Interruptible.
    Preparing,
    /// Byte transfer in progress. Interruptible (pause/cancel).
    Downloading,
    /// Signature / integrity verification. Interruptible.
    Verifying,
    /// Reconstruct / unpack / apply delta into staging. Interruptible until commit.
    Applying,
    /// Destructive renames (old → backup, new → install). Point of no return.
    Committing,
    /// Post-swap health check, provenance, relaunch. Non-interruptible.
    Finishing,
}

impl OperationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Preparing => "preparing",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Applying => "applying",
            Self::Committing => "committing",
            Self::Finishing => "finishing",
        }
    }

    /// Safe phases may be paused, cancelled, or quit-after-cancel.
    pub fn interruptible(self) -> bool {
        !self.is_point_of_no_return()
    }

    /// Destructive rename tail + post-commit bookkeeping must not be aborted
    /// mid-flight by process exit.
    pub fn is_point_of_no_return(self) -> bool {
        matches!(self, Self::Committing | Self::Finishing)
    }

    /// Stable machine code for frontend i18n (`close.blocked.reason.*`).
    pub fn reason_code(self, kind: Option<OperationKind>) -> &'static str {
        match (self, kind.is_none()) {
            (Self::Committing, _) => "committing",
            (Self::Finishing, true) => "other-process",
            (Self::Finishing, false) => "finishing",
            _ => "busy",
        }
    }

    pub fn user_reason(self, kind: Option<OperationKind>) -> String {
        let kind_label = kind.map(|k| k.as_str()).unwrap_or("operation");
        match self {
            Self::Committing => format!(
                "正在执行关键文件替换（{kind_label}），强行退出可能导致安装不完整。请稍候完成。"
            ),
            Self::Finishing if kind.is_none() => {
                "另一个管理器实例正在执行操作，请稍候完成后再关闭。".to_string()
            }
            Self::Finishing => format!("正在完成安装收尾（{kind_label}），请稍候完成后再关闭。"),
            Self::Downloading => format!("正在下载（{kind_label}），关闭将取消或暂停本次操作。"),
            Self::Preparing | Self::Verifying | Self::Applying => {
                format!("正在准备更新（{kind_label}），关闭将取消本次操作。")
            }
            Self::Idle => "当前没有进行中的操作。".to_string(),
        }
    }
}

/// Backend decision for window close / menu quit / quit command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "decision")]
pub enum QuitPolicy {
    /// Exit immediately (no active non-interruptible work; confirm_close off or force).
    Allow,
    /// Raise the normal close-confirm dialog.
    Confirm,
    /// Refuse exit and tell the user why (point of no return).
    Block {
        phase: OperationPhase,
        /// Stable code for i18n (`committing` / `finishing` / `other-process`).
        reason_code: String,
        /// Human-readable fallback (Chinese log / older clients).
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
}

impl QuitPolicy {
    pub fn evaluate(
        force_quit: bool,
        confirm_close: bool,
        busy: bool,
        phase: OperationPhase,
        kind: Option<OperationKind>,
    ) -> Self {
        if force_quit {
            return Self::Allow;
        }
        if busy && phase.is_point_of_no_return() {
            return Self::Block {
                phase,
                reason_code: phase.reason_code(kind).to_string(),
                reason: phase.user_reason(kind),
                kind: kind.map(|k| k.as_str().to_string()),
            };
        }
        if confirm_close {
            Self::Confirm
        } else {
            Self::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationPhase, QuitPolicy};
    use crate::app::oplock::OperationKind;

    #[test]
    fn phases_mark_commit_and_finish_as_point_of_no_return() {
        assert!(OperationPhase::Preparing.interruptible());
        assert!(OperationPhase::Downloading.interruptible());
        assert!(OperationPhase::Verifying.interruptible());
        assert!(OperationPhase::Applying.interruptible());
        assert!(!OperationPhase::Committing.interruptible());
        assert!(!OperationPhase::Finishing.interruptible());
        assert!(OperationPhase::Committing.is_point_of_no_return());
        assert!(OperationPhase::Finishing.is_point_of_no_return());
    }

    #[test]
    fn quit_policy_blocks_only_point_of_no_return() {
        let blocked = QuitPolicy::evaluate(
            false,
            false, // confirm_close off still cannot force through commit
            true,
            OperationPhase::Committing,
            Some(OperationKind::Update),
        );
        assert!(matches!(blocked, QuitPolicy::Block { .. }));

        let allow_download = QuitPolicy::evaluate(
            false,
            false,
            true,
            OperationPhase::Downloading,
            Some(OperationKind::Update),
        );
        assert_eq!(allow_download, QuitPolicy::Allow);

        let confirm_download = QuitPolicy::evaluate(
            false,
            true,
            true,
            OperationPhase::Downloading,
            Some(OperationKind::Update),
        );
        assert_eq!(confirm_download, QuitPolicy::Confirm);

        let force = QuitPolicy::evaluate(
            true,
            true,
            true,
            OperationPhase::Committing,
            Some(OperationKind::Update),
        );
        assert_eq!(force, QuitPolicy::Allow);
    }
}
