# AWAI Codex App Manager

本版本没有单独的 release note，以下是安全的通用安装说明。版本、平台和签名状态以
GitHub Release Assets 与 `SHA256SUMS` 为准。

## 下载

最新 Manager 工件位于 [codexapp.awai.cc](https://codexapp.awai.cc)：

| 平台 | 下载 |
| --- | --- |
| Windows x64 | `https://codexapp.awai.cc/manager/latest/CodexAppManager_x64-setup.exe` |
| Windows ARM64 | `https://codexapp.awai.cc/manager/latest/CodexAppManager_arm64-setup.exe` |
| macOS Apple Silicon | `https://codexapp.awai.cc/manager/latest/CodexAppManager_aarch64.dmg` |
| macOS Intel | `https://codexapp.awai.cc/manager/latest/CodexAppManager_x86_64.dmg` |

镜像直链始终指向最新版本；需要精确历史版本时请使用该版本的 GitHub Release Assets。

## 安装前核验

下载同一版本的 `SHA256SUMS` 并核对：

```powershell
Get-FileHash .\CodexAppManager_x64-setup.exe -Algorithm SHA256
```

macOS 使用 `shasum -a 256 <file>`。Windows 当前预览安装器可能没有 Authenticode，
SmartScreen 警告并不表示 Tauri updater 签名失效；请先看
[`windows-signing.md`](../windows-signing.md)。

Manager 安装后负责后续 Codex 检查、配置和皮肤操作。API 配置默认使用
`https://api.awai.cc/v1`，API Key 只保存在本机。

隐私政策：[`privacy.md`](../privacy.md)。许可证：[`LICENSE`](../../LICENSE)。
