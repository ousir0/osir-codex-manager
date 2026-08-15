<!--
  Copy this file to docs/releases/vX.Y.Z.md for a release.
  Write only facts verified by CI or a reproducible local check.
  Keep Chinese and English together; do not copy historical upstream text.
-->

# AWAI Codex App Manager vX.Y.Z

一句话说明用户能感知的变化。

## 亮点 · Highlights

- **功能名称**：中文说明和验证方式。
  English description and how users can verify it.

## 修复 · Fixes

- **问题**：之前的症状 → 当前行为。
  What was broken and what happens now.

## 下载 · Download

| 平台 | 下载 |
| --- | --- |
| Windows x64 | [GitHub Release Assets](https://github.com/qq501987847/codex-app-manager/releases) |
| Windows ARM64 | [GitHub Release Assets](https://github.com/qq501987847/codex-app-manager/releases) |
| macOS Apple Silicon | [GitHub Release Assets](https://github.com/qq501987847/codex-app-manager/releases) |
| macOS Intel | [GitHub Release Assets](https://github.com/qq501987847/codex-app-manager/releases) |

最新版本镜像直链：<https://codexapp.awai.cc>。历史版本请使用本页对应的 Assets，不要
把 `/latest/` 链接当成固定版本。

## 核验与已知限制 · Verification and known limitations

- 下载同一 Release 的 `SHA256SUMS`，按 [`windows-signing.md`](../windows-signing.md)
  或平台工具核对哈希。
- Windows Authenticode 状态必须据实填写；不要把 Tauri `.sig` 写成 Windows 发行者签名。
- macOS Developer ID、notarization 和 delta 状态只写 CI 实际验证过的结果。
- 隐私说明见 [`privacy.md`](../privacy.md)。
