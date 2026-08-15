<p align="center">
  <img src="./assets/banner.svg" alt="AWAI Codex App Manager" width="100%">
</p>

<p align="center">
  <img src="./assets/logo.png" width="160" alt="AWAI Codex App Manager logo">
</p>

<h1 align="center">AWAI Codex App Manager</h1>

<p align="center">
  Windows / macOS 上的 Codex 安装、更新、配置与主题管理器。<br>
  A local manager for installing, updating, configuring, and theming Codex on Windows and macOS.
</p>

<p align="center">
  <a href="https://github.com/qq501987847/codex-app-manager/releases/latest"><img src="https://img.shields.io/github/v/release/qq501987847/codex-app-manager?logo=github&label=release" alt="Latest release"></a>
  <a href="https://github.com/qq501987847/codex-app-manager/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/qq501987847/codex-app-manager/ci.yml?branch=main&label=CI&logo=githubactions" alt="CI workflow"></a>
  <a href="https://codexapp.awai.cc"><img src="https://img.shields.io/badge/download-codexapp.awai.cc-69A5FA" alt="AWAI download mirror"></a>
  <a href="https://v2.tauri.app/"><img src="https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white" alt="Tauri v2"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://github.com/qq501987847/codex-app-manager"><b>GitHub</b></a> ·
  <a href="https://gitee.com/qq501987849/codex-app-manager"><b>Gitee</b></a> ·
  <a href="https://codexapp.awai.cc"><b>下载镜像</b></a> ·
  <a href="https://api.awai.cc"><b>AWAI API</b></a> ·
  <a href="docs/code-signing-policy.md">代码签名政策</a> ·
  <a href="docs/privacy.md">隐私政策</a> ·
  <a href="#readme-en">English</a>
</p>

---

<div align="center">
<table>
  <tr>
    <td align="center" width="170">
      <a href="https://api.awai.cc"><img src="./assets/awai-avatar-blue-1024.png" alt="AWAI API" width="96"></a>
    </td>
    <td width="560">
      <b>本项目由 <a href="https://api.awai.cc">AWAI API 中转站</a> 提供服务支持</b><br>
      为 Codex 提供兼容 OpenAI API 的统一入口，默认地址为 <code>https://api.awai.cc/v1</code>。<br>
      <b>Powered by <a href="https://api.awai.cc">AWAI API</a></b> — one compatible endpoint for your Codex configuration.
    </td>
  </tr>
</table>
</div>

---

<a id="readme-cn"></a>

# 中文

AWAI Codex App Manager 是一个 Tauri 桌面应用，用来管理官方 Codex 的本地安装生命周期：
检测、安装、更新、配置、主题、启动和卸载。它通过自有的 `codexapp.awai.cc` 镜像提供
可达下载，并用 SHA-256、包身份和平台原生签名验证每一个工件。

Manager 不修改 Codex 应用包，不绕过 OpenAI 或 Microsoft 的授权与安装策略，也不会把
你的 API Key、对话或工作区上传到 AWAI。

## 能力一览

| 能力 | 说明 |
|---|---|
| 🧭 **一站式管理** | 检测本机 Codex，生成安装 / 更新 / 卸载计划，确认后执行并展示结果 |
| 🪟 **Windows 双路径** | 优先安装官方 MSIX；环境不满足时回退到经过校验的便携安装 |
| 🍎 **macOS 增量更新** | 读取 Sparkle appcast，优先使用 delta；EdDSA 或替换失败时回退完整包 |
| ⚙️ **CODEX 配置管理** | API Key、Base URL、模型获取、Goal Mode、响应存储、人格和安全策略 |
| 🎨 **AWAI 皮肤库** | 导入、预览、试穿、应用和恢复 `.codexskin`，不改写 Codex 安装文件 |
| 🔄 **Manager 自更新** | Tauri updater 读取 AWAI `latest.json`，签名校验通过后由用户确认安装 |
| 🌐 **浏览器开发预览** | 不启动 Tauri 也能先验证界面、状态和配置流程 |
| 🛡️ **可核验分发** | HTTPS、SHA-256、Windows MSIX / macOS Developer ID 与 updater 签名分层校验 |

## 下载与安装

### 直接下载

从 [GitHub Releases](https://github.com/qq501987847/codex-app-manager/releases/latest) 获取
全球发布记录，或使用 [AWAI 镜像](https://codexapp.awai.cc) 的固定最新链接：

| 平台 | 文件 | AWAI 镜像 |
|---|---|---|
| Apple Silicon Mac | `CodexAppManager_aarch64.dmg` | [下载](https://codexapp.awai.cc/manager/latest/CodexAppManager_aarch64.dmg) |
| Intel Mac | `CodexAppManager_x86_64.dmg` | [下载](https://codexapp.awai.cc/manager/latest/CodexAppManager_x86_64.dmg) |
| Windows x64 | `CodexAppManager_x64-setup.exe` | [下载](https://codexapp.awai.cc/manager/latest/CodexAppManager_x64-setup.exe?build=20260815-logo) |
| Windows ARM64 | `CodexAppManager_arm64-setup.exe` | [下载](https://codexapp.awai.cc/manager/latest/CodexAppManager_arm64-setup.exe) |

镜像的 `/manager/latest/` 始终指向当前版本；需要精确历史版本时，请使用对应 GitHub
Release 的 Assets。安装 Manager 后，Codex 本体由 Manager 负责安装与更新，不需要另找
单独的 Codex 安装包。

### 安装前核验

每个发布包含 `SHA256SUMS`。Windows PowerShell：

```powershell
Get-FileHash .\CodexAppManager_x64-setup.exe -Algorithm SHA256
# ARM64 替换为 CodexAppManager_arm64-setup.exe
```

macOS：

```bash
shasum -a 256 CodexAppManager_aarch64.dmg
```

Windows 预览安装器当前可能没有 Authenticode，首次运行出现 SmartScreen 提示属于已披露
限制。Tauri updater 的 `.sig` 只验证更新字节，不等同于 Windows 发行者签名。请先阅读
[Windows 签名与核验](docs/windows-signing.md) 和 [代码签名政策](docs/code-signing-policy.md)。

## Manager 自更新

Manager 先检查 `https://codexapp.awai.cc/manager/latest.json`，再按配置使用 GitHub
后备源。`latest.json` 的签名绑定最终安装包字节，AWAI 镜像只复制这些字节并改写 URL。
发现更新、下载、安装和重启都需要用户确认；镜像不可用时不会跳过签名验证。

## Codex 配置管理

左侧 **CODEX 配置管理** 页面集中维护 Codex 的 `config.toml` 与凭据状态：

- 默认 Provider 为 `awai`，Base URL 为 `https://api.awai.cc/v1`；也可以改为其他兼容 API。
- 默认模型为 `gpt-5.6-sol`，使用“获取模型”从当前 Base URL 的 `/models` 读取列表。
- API Key 只显示“已配置/未配置”，写入本机凭据文件，不进入皮肤包、日志或 Git。
- 支持 `goal_mode`、`disable_response_storage`、`personality`、`approval_policy` 和
  `sandbox_mode` 等字段，并在写入后重新解析验证。
- `approval_policy = "never"` 或 `sandbox_mode = "danger-full-access"` 会显示风险提示；
  Manager 不会在用户不知情时启用危险模式。

## AWAI 主题皮肤

皮肤仓库：

- [GitHub · codex-app-manager-skins](https://github.com/qq501987847/codex-app-manager-skins)
- [Gitee · codex-app-manager-skins](https://gitee.com/qq501987849/codex-app-manager-skins)

皮肤是视觉样式和背景素材包，不是配置包。支持在线目录、导入 `.codexskin`、预览、运行
中试穿、持久应用和恢复默认。应用失败会恢复上一个快照，整个过程不修改 Codex 二进制、
`app.asar` 或签名。

请只使用自己生成、自己拍摄或明确获得授权的图片。MIT 许可覆盖软件代码，不会自动覆盖
第三方人物图、字体、音乐或其他素材。

## 工作原理

### 检测 → 规划 → 执行

Manager 先读取本机安装状态、版本、来源和平台能力，生成可读计划；用户确认后才下载、
暂存和替换。下载完成后依次检查 HTTPS、manifest、SHA-256、包身份和平台原生签名，
任何失败都停留在暂存区，不触碰当前可运行版本。

### AWAI 分发链路

`codexapp.awai.cc` 提供 Windows manifest / checksums、macOS Sparkle appcast 和 Manager
自更新入口。服务器使用 HTTPS 和版本化文件路径；发布后从公开 URL 回读并重新计算哈希，
确认 GitHub Release、镜像和 `latest.json` 使用同一批字节。

### 发布流水线

推送受保护的 `vX.Y.Z` tag 后，GitHub Actions 构建四个平台，运行 TypeScript / Rust 测试
和安装冒烟；macOS 按嵌套二进制到 App 的顺序签名和公证，Windows 按实际证书状态生成
Authenticode 与 updater 工件，然后同步到 AWAI 镜像。

## 技术栈与本地开发

- **前端**：React 19 · TypeScript · Vite · GSAP
- **外壳**：Tauri v2
- **后端**：Rust 命令层、应用服务、平台适配；`codex-mac-engine` 与 `codex-win-engine`

```bash
npm install
npm run check
npm test
npm run tauri:dev
```

构建 Windows NSIS 安装器：

```bash
npm run tauri build -- --bundles nsis --target x86_64-pc-windows-msvc
```

本地构建默认未配置 Authenticode、Apple notarization 或 Tauri updater 私钥；正式发布必须
在受保护环境中配置对应凭据。

## 边界声明

- 不修改、不重新打包官方 Codex 安装包。
- 不绕过 OpenAI / Microsoft 授权、Windows 策略或 macOS Gatekeeper。
- 不伪造或重算官方 Sparkle / MSIX 签名。
- 不上传对话、工作区、API Key 或用户配置，不运行项目自营遥测。
- 不把未签名预览包描述为稳定发行版。

## 政策与许可证

- [代码签名政策](docs/code-signing-policy.md)
- [Windows 签名与核验](docs/windows-signing.md)
- [隐私政策](docs/privacy.md)
- [发布指南](docs/release.md)
- [镜像接口契约](docs/manifest-contract.md)

本项目保留根目录 [`LICENSE`](LICENSE) 中的 MIT 原版权声明与许可文本，允许修改、再发布
和商用。本项目与 OpenAI、Microsoft 没有隶属或背书关系。

---

<a id="readme-en"></a>

# English

AWAI Codex App Manager is a Tauri desktop manager for the official Codex app. It covers local
install, update, configuration, skin, launch, and uninstall workflows on Windows and macOS.
It serves current artifacts from `https://codexapp.awai.cc`, validates them before touching an
installation, and keeps API credentials and user content on the local machine.

## At a glance

| Capability | Detail |
|---|---|
| One-stop management | Detect, plan, confirm, execute, and report install / update / uninstall operations |
| Windows paths | Prefer the official MSIX; fall back to a verified portable install when required |
| macOS updates | Read the Sparkle appcast, prefer signed deltas, and fall back to the full archive |
| CODEX configuration | AWAI Base URL, API key state, model discovery, Goal Mode, storage and safety fields |
| AWAI skins | Import, preview, try on, apply, and restore `.codexskin` packages without editing Codex |
| Manager self-update | Tauri updater with `https://codexapp.awai.cc/manager/latest.json` and a GitHub fallback |
| Verification | HTTPS, SHA-256, package identity, native platform signatures, and post-install health checks |

## Download & verify

Use the [latest GitHub Release](https://github.com/qq501987847/codex-app-manager/releases/latest)
for the public release record, or the fixed AWAI mirror links:

| Platform | Mirror |
|---|---|
| Apple Silicon Mac | [CodexAppManager_aarch64.dmg](https://codexapp.awai.cc/manager/latest/CodexAppManager_aarch64.dmg) |
| Intel Mac | [CodexAppManager_x86_64.dmg](https://codexapp.awai.cc/manager/latest/CodexAppManager_x86_64.dmg) |
| Windows x64 | [CodexAppManager_x64-setup.exe](https://codexapp.awai.cc/manager/latest/CodexAppManager_x64-setup.exe?build=20260815-logo) |
| Windows ARM64 | [CodexAppManager_arm64-setup.exe](https://codexapp.awai.cc/manager/latest/CodexAppManager_arm64-setup.exe) |

`/manager/latest/` always means the newest release. Use the matching GitHub Release Assets for
an exact historical version. Verify `SHA256SUMS` before running an installer. Preview Windows
installers may be unsigned by Authenticode; a Tauri updater signature authenticates update bytes,
not Windows publisher identity. See the [code signing policy](docs/code-signing-policy.md) and
[Windows signing guide](docs/windows-signing.md).

## Configuration and skins

The **CODEX configuration** page defaults to `https://api.awai.cc/v1` and `gpt-5.6-sol`. It can
fetch models from `/models`, stores the API key in Codex's local credential store, and edits the
supported TOML fields with validation. Risky settings are shown explicitly before saving.

The [AWAI skin repository](https://github.com/qq501987847/codex-app-manager-skins) provides
`.codexskin` packages for visual styles and authorized artwork only. A skin never contains an
API key, API endpoint, or user content, and applying one never rewrites the Codex installation.

## Development

```bash
npm install
npm run check
npm test
npm run tauri:dev
```

The project uses React / TypeScript / Vite on the frontend and Tauri v2 / Rust on the backend.
The macOS and Windows engines keep platform verification and rollback logic outside the UI.

## Scope and license

AWAI Codex App Manager does not patch Codex, bypass platform policies, forge official signatures,
upload conversations or credentials, or operate project-owned telemetry. The repository is an
MIT-licensed fork; the original copyright and license text in [`LICENSE`](LICENSE) remain intact.
It is independent from and not endorsed by OpenAI or Microsoft.
