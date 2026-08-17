# Codex App Manager 服务与云端位置清单

核对日期：2026-08-17

## 结论

项目的线上依赖主要分为六组：自建下载与更新站、API 中转、GitHub/Gitee 内容托管、OpenAI 官方服务、GitHub Actions 发布系统，以及尚未完全投入线上流量的 Cloudflare R2/IHEP S3 分发方案。

当前最核心的入口是 `codexapp.awai.cc`。它同时承担网站、Windows 安装源、macOS 镜像源、Manager 自更新文件和国际化隧道。实测该域名目前直接解析到美国洛杉矶的 VPS，并由 Caddy 提供服务。

> 云厂商和地区来自 2026-08-17 的 DNS、HTTP 响应和 IP/ASN 数据。CDN 边缘节点会随访问位置变化，地理位置不应理解为永久固定。

## 运行时服务清单

| 服务 | 访问位置 | 用途 | 当前云端/托管位置 | 当前状态 | 代码位置 |
|---|---|---|---|---|---|
| 主站与统一下载入口 | `https://codexapp.awai.cc/` | 官网、下载入口和统一域名 | `64.83.13.186`，AS979 NetLab Global，美国洛杉矶；Caddy 服务器 | 实测 HTTP 200 | `deploy/download-server/Caddyfile`、`website/` |
| Manager 自更新清单 | `https://codexapp.awai.cc/manager/latest.json` | 告知客户端最新 Manager 版本和安装包地址 | 当前与主站相同，为洛杉矶 VPS/Caddy | 实测 HTTP 200 | `src-tauri/tauri.conf.json` |
| Manager 自更新备用源 | `https://github.com/qq501987847/codex-app-manager/releases/latest/download/latest.json` | 主更新源不可用时读取 GitHub Release | GitHub 云端；网页请求实测走 Microsoft/GitHub 新加坡节点，Release 文件由 GitHub 对象存储/CDN 分发 | 最新 Release 为 `v0.5.3` | `src-tauri/tauri.conf.json` |
| Windows 版本清单 | `https://codexapp.awai.cc/latest/manifest` | 查询可安装的 Windows Codex 版本、架构和包信息 | 洛杉矶 VPS/Caddy | 实测 HTTP 200 | `src-tauri/src/state.rs`、`src-tauri/src/domain/manifest.rs` |
| Windows 校验清单 | `https://codexapp.awai.cc/latest/checksums` | 校验下载包 SHA-256 | 洛杉矶 VPS/Caddy | 实测 HTTP 200 | `src-tauri/src/domain/manifest.rs` |
| Windows 安装包 | `/latest/win`、`/latest/win-x64`、`/latest/win-arm64`、`/latest/win-unpacked` | 下载 MSIX 或便携包 | 当前由洛杉矶 VPS/Caddy 文件服务提供 | 在线，实际速度受服务器出口限制 | `src-tauri/src/domain/manifest.rs`、`src-tauri/src/app/win_update.rs` |
| macOS 镜像 Appcast | `/latest/appcast.xml`、`/latest/appcast-x64.xml` | 查询 macOS Codex 更新、完整包和增量包 | 洛杉矶 VPS/Caddy | 在线 | `src-tauri/src/app/mac_update.rs` |
| OpenAI 官方 macOS Appcast | `https://persistent.oaistatic.com/codex-app-prod/appcast*.xml` | macOS 官方更新源；自动模式会与镜像比较版本 | Cloudflare CDN 前置，响应显示后端为 Microsoft Azure Blob Storage | 实测 HTTP 200 | `src-tauri/src/app/mac_update.rs` |
| API 中转 | `https://api.awai.cc/v1` | Codex、Claude Code、Gemini 等兼容 API；模型列表访问 `/v1/models` | `38.58.58.108`，AS979 NetLab Global，美国洛杉矶；OpenResty | 实测返回 HTTP 401，说明服务在线且需要 API Key | `src/app/views/CodexConfig.tsx`、`src/services/managerApi.ts` |
| OpenAI API 默认源 | `https://api.openai.com/v1` | 未填写自定义 Base URL 时获取模型列表 | OpenAI 官方 API；具体边缘和后端由 OpenAI 网络调度 | 当前网络环境请求超时，不能据此判断全局服务状态 | `src-tauri/src/app/codex_config.rs` |
| 在线皮肤主源 | `https://raw.githubusercontent.com/qq501987847/codex-app-manager-skins/main` | 获取 `index.json`、预览图和 `.codexskin` 包 | GitHub Raw + Fastly CDN；本次实测命中日本东京/日本东部边缘 | 实测 HTTP 200 | `src-tauri/src/app/codex_theme.rs`、`vite.config.ts` |
| 在线皮肤备用源 | `https://gitee.com/qq501987849/codex-app-manager-skins/raw/main` | GitHub Raw 不可用时回退 | Gitee，下载会跳转到 `raw.giteeusercontent.com`；DNS/响应显示使用百度 CDN，中国节点 | 实测 HTTP 302 后进入 Raw 文件服务 | `src-tauri/src/app/codex_theme.rs`、`vite.config.ts` |
| 历史 Codex 包目录 | `https://api.github.com/repos/Wangnov/codex-app-mirror/releases...` | 查询历史 macOS DMG/ZIP 和 Windows MSIX | GitHub API + GitHub Releases | 仓库公开可访问 | `src-tauri/src/app/release_install.rs` |
| 历史 Codex 包文件 | `https://github.com/Wangnov/codex-app-mirror/releases/download/...` | 下载用户选定的历史安装包 | GitHub Release 对象存储/CDN | 由代码校验仓库路径、架构、摘要和包身份 | `src-tauri/src/app/release_install.rs` |
| 国际化 WebSocket 隧道 | `wss://codexapp.awai.cc/i18n-tunnel` | 将特定国际化请求转发到 `ab.chatgpt.com:443` | 公网入口在洛杉矶 VPS/Caddy；Caddy 转发到服务器本机 `127.0.0.1:3130` | 路径由 Caddy 反向代理 | `crates/codex-win-engine/src/i18n_proxy.rs`、`deploy/awai-i18n-relay/` |
| 国际化上游 | `ab.chatgpt.com:443` | Codex UI 国际化启动请求 | OpenAI/ChatGPT 官方服务 | 当前网络环境请求超时；隧道设计为端到端 TLS，不在中间解密内容 | `deploy/awai-i18n-relay/awai_i18n_ws.py` |

## `codexapp.awai.cc` 的路径分工

| 路径 | 功能 | 后端位置 |
|---|---|---|
| `/` | 官网静态页面 | 当前为 VPS `/srv/awai/site` |
| `/latest/manifest` | Windows 版本元数据 | 当前为 VPS `/srv/awai/latest` |
| `/latest/checksums` | Windows 文件校验值 | 当前为 VPS `/srv/awai/latest` |
| `/latest/win*` | Windows 安装文件 | 当前为 VPS `/srv/awai/latest` |
| `/latest/appcast*.xml` | macOS Sparkle 更新清单 | 当前为 VPS `/srv/awai/latest` |
| `/manager/latest.json` | Manager 自更新清单 | 当前为 VPS `/srv/awai/manager` |
| `/manager/<version>/...` | Manager 各平台安装包和更新包 | 当前为 VPS `/srv/awai/manager` |
| `/i18n-tunnel` | 国际化 WebSocket 隧道 | Caddy → 服务器本机 `127.0.0.1:3130` → `ab.chatgpt.com:443` |

## Cloudflare R2 与 IHEP S3：代码设计和当前实测的差异

仓库中已经提供一套更完整的 Manager 安装包分发设计：

1. `codexapp.awai.cc/manager/*` 进入 Cloudflare Worker。
2. 全球请求从 Cloudflare R2 bucket `codex-app-manager` 读取。
3. 中国大陆请求可重定向到 IHEP S3 兼容对象存储。
4. GitHub Actions 发布时同时上传 GitHub Release、R2 和 IHEP S3，并回读校验。

相关位置：

- `cloudflare/manager-download-router/wrangler.jsonc`
- `cloudflare/manager-download-router/src/index.js`
- `.github/workflows/release.yml`
- `scripts/sync-mirror.sh`

设计中的云端：

| 组件 | 位置 | 说明 |
|---|---|---|
| Cloudflare Worker | Cloudflare 全球边缘网络 | 按国家/地区选择 R2 或 IHEP S3 |
| Cloudflare R2 | `d39dc6c92d1c4cfde580bf13e946b616.r2.cloudflarestorage.com` | Manager 发布包的全球对象存储；实测为 Cloudflare 网络 |
| IHEP S3 | 示例端点 `https://s3.ihep.ac.cn` | 中国大陆备用对象存储，实际 endpoint、bucket 和密钥放在 GitHub Secrets/Variables |

当前实测结果与上述设计不同：

- `codexapp.awai.cc` 直接解析为 `64.83.13.186`，不是 Cloudflare Anycast 地址。
- HTTP 响应头为 `Server: Caddy`，没有 `Server: cloudflare`。
- 强制请求 `r2`/`ihep` 探测分支时，没有返回 Worker 代码定义的 `X-Codex-Mirror-Backend`。

因此，2026-08-17 的有效线上拓扑应按“VPS/Caddy 直出”理解；Cloudflare Worker + R2 + IHEP S3 更像是仓库中已准备但当前域名流量尚未实际使用的发布架构。

## 构建与发布云服务

| 服务 | 云端位置 | 用途 |
|---|---|---|
| GitHub Actions Ubuntu Runner | GitHub 托管 Linux Runner | 发布预检、工件收集、清单生成、镜像同步 |
| GitHub Actions Windows Runner | `windows-latest` | 构建 Windows x64/ARM64、NSIS、安装冒烟测试 |
| GitHub Actions macOS Runner | `macos-latest`、`macos-15-intel` | 构建 Apple Silicon/Intel 包、签名、公证 |
| GitHub Releases | GitHub 云端 | 保存正式安装包、`latest.json`、校验文件和发布记录 |
| Cloudflare R2 | Cloudflare 对象存储 | 设计中的全球 Manager 工件镜像 |
| IHEP S3 | 中国科学院高能物理研究所相关 S3 兼容存储 | 设计中的中国大陆下载分支 |
| Apple 服务 | Apple Developer ID / Notary Service | macOS 代码签名和公证；凭据存在 GitHub protected environment |
| Windows 代码签名 | Authenticode 证书与时间戳服务 | 可选的 Windows 发布者签名；当前文档说明可能未配置 |

## 本机服务，不属于云端

| 地址/端口 | 用途 |
|---|---|
| `127.0.0.1:1420` | Vite 前端开发预览 |
| `127.0.0.1:19443` | Windows 本地 PAC/CONNECT 代理，只允许国际化目标 |
| `127.0.0.1:9345` | Codex Chromium DevTools/CDP，用于主题注入 |
| `tauri://localhost`、`ipc.localhost`、`asset.localhost` | Tauri WebView 内部协议，不经过公网 |
| 用户配置的 `127.0.0.1:7890` 等 | 可选本机 HTTP/SOCKS 代理，不是项目固定服务 |

## 用户可配置的外部服务

以下位置不是固定云端，最终流量去向取决于用户填写内容：

- 自定义 Codex 更新源
- 自定义 API Base URL
- 生图 API Base URL
- MCP HTTP 服务
- HTTP、SOCKS5、SOCKS5H 网络代理

API Key 会发送给当前选择的 API Base URL；安装包和更新文件会从当前选择的更新源下载。更改这些字段前，应确认域名、证书、文件校验和服务控制权。

## 依赖影响摘要

| 服务不可用 | 影响 |
|---|---|
| `codexapp.awai.cc` | 官网、Windows 安装/更新、macOS 镜像、Manager 自更新主源、国际化隧道同时受影响 |
| `api.awai.cc` | 使用该 Base URL 的模型请求、生图兼容请求和模型列表获取失败 |
| GitHub | Manager 更新备用源、历史版本、皮肤主源和发布流水线受影响 |
| Gitee | 仅皮肤备用源受影响；GitHub 主源正常时影响有限 |
| `persistent.oaistatic.com` | macOS 官方更新源不可用，自动模式可能退回镜像 |
| `ab.chatgpt.com` 或国际化隧道 | Codex 国际化初始化可能回退直连或失败，不影响其他普通域名的直连 |

## 建议的后续拆分顺序

1. 先保留现有访问路径，避免功能立即中断。
2. 将 Manager 自更新、Codex 包镜像、API 中转、皮肤目录拆成四组独立配置。
3. 优先迁移 Manager 自更新，因为它决定客户端自身接收哪套安装包。
4. 再迁移皮肤目录和 API Base URL；它们与安装器构建相互独立。
5. 最后决定是否启用 Cloudflare Worker + R2 + IHEP S3 的正式分流。
