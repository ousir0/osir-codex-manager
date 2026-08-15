# 隐私政策 · Privacy Policy

生效日期 / Effective date: 2026-08-15

本政策适用于 AWAI Codex App Manager、`codexapp.awai.cc` 下载服务和本仓库维护的公开
页面。用户主动配置的第三方 API、代理、GitHub、Gitee、OpenAI 和 Microsoft 服务，
还分别适用其自身政策。

## 我们不收集什么

- 不提供账户系统，不要求姓名、邮箱、电话或付款信息。
- 不运行遥测、广告、行为分析或自动崩溃上报服务。
- 不主动读取或上传对话、工作区文件、`~/.codex` 内容或 API Key。
- 不把本机配置、日志或诊断自动上传到 AWAI；用户主动复制到 Issue 时自行负责脱敏。

## 应用会访问什么

网络请求只为用户可见的功能服务：

- 启动或手动检查 Codex 版本时访问 `codexapp.awai.cc` 的 manifest、appcast 和校验清单。
- 用户确认安装或更新后下载官方 Codex 工件。
- 用户在“关于”页面点击检查时读取 `manager/latest.json`；下载和重启需要再次确认。
- 用户点击“获取模型”时，使用已保存 API Key 请求当前 Base URL 的 `/models`。
- 用户打开仓库、反馈或政策链接时才访问外部网页。

请求可能经过系统代理、用户配置的代理或直连。网络服务通常会看到 IP、时间、路径、
User-Agent、状态码和安全日志；这些是服务端正常运行所需的元数据，不是项目应用层遥测。

Windows 安装器在系统没有可用 WebView2 Runtime 时，可能访问 Microsoft 的 bootstrapper
地址；已有 Runtime 的系统不会重复下载。

## 本地保存的数据

应用会在本机保存：

- Codex 配置和凭据文件（API Key 由 Codex 的凭据存储负责保存）；
- 安装来源、版本、provenance、操作状态和回滚材料；
- 语言、代理、更新检查和界面偏好；
- 运行日志、校验结果和错误信息。

诊断报告可能包含应用版本、系统架构、更新源 host、安装状态、路径和错误文本。复制前
请检查并删除 token、密码、私钥、完整预签名 URL 以及工作区路径等敏感内容。

卸载 Manager 不会自动删除 Codex 自己拥有的配置、凭据或工作区数据；卸载界面会说明
保留与清除范围。

## 皮肤与 API Key 边界

`.codexskin` 只包含主题样式和允许分发的图片素材，不包含 API 地址、API Key、对话或
用户文件。应用皮肤时数据留在本机，皮肤仓库不会收到用户配置。

AWAI API 的请求由用户在配置页主动启用。API Key 不会作为查询参数发送，也不会写进皮肤
包或公开仓库；但 API 服务本身仍可能按其政策记录请求元数据。

## 联系方式

请通过新仓库的 [GitHub Issues](https://github.com/qq501987847/codex-app-manager/issues)
报告隐私或安全问题。不要在公开 Issue 中粘贴凭据、私钥、完整预签名 URL 或对话内容。

本政策随功能或数据流变化更新；仓库中的版本记录和代码是具体实现的最终依据。

## English summary

AWAI Codex App Manager has no account, telemetry, advertising, or project-operated crash
reporting. It keeps settings, credentials, provenance, logs, and diagnostics on the local
machine. Network requests are limited to update/download checks, user-confirmed installs,
model discovery, and links explicitly opened by the user. API keys and Codex content are not
uploaded by the manager and are not included in skin packages. Review and redact diagnostics
before sharing them publicly.
