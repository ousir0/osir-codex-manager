# AWAI Codex App Manager 产品设计

## 产品目标

AWAI Codex App Manager 是 Windows 和 macOS 上的本地桌面管理器。它把“安装、更新、
配置、主题、卸载”集中到一个可验证的界面中，降低受限网络和多版本安装带来的维护成本。

Manager 管理官方 Codex 的安装生命周期，但不重新打包、不修改 Codex 二进制，也不代表
OpenAI 或 Microsoft。官方包由 `codexapp.awai.cc` 提供可达镜像，包自身的系统签名仍是
主要信任锚。

## 目标用户与首要任务

1. 在 Windows 上下载并安装当前官方 Codex，遇到 MSIX 条件不满足时使用校验过的便携路径。
2. 在 macOS 上优先使用 Sparkle delta 更新，失败时自动回退完整包。
3. 在 `CODEX 配置管理` 页面设置 `api.awai.cc/v1`、API Key、模型和安全策略。
4. 在不修改 Codex 安装文件的情况下预览、应用或恢复 AWAI 皮肤。
5. 出问题时查看可脱敏的操作结果和诊断信息，而不是重新猜测下载是否成功。

## 界面信息架构

左侧导航保持固定且按工作频率排序：

- **概览**：安装状态、当前版本、更新可用性和最近一次操作。
- **Codex 配置管理**：Provider、Base URL、API Key 状态、模型获取、Goal Mode、
  `disable_response_storage`、`personality`、`approval_policy`、`sandbox_mode`。
- **皮肤**：在线目录、导入 `.codexskin`、试穿、应用和恢复默认。
- **设置**：更新检查、代理、语言、诊断和危险操作确认。
- **关于**：版本、许可证、仓库、镜像状态和自更新检查。

所有有副作用的按钮都先展示目标、来源、版本、预计空间和失败回滚方式。下载、安装、
配置保存和皮肤应用都要有进行中、成功、失败和可重试状态；不能只用一个转圈图标表示未知状态。

## 配置管理原则

- 默认 Provider 为 `awai`，默认 Base URL 为 `https://api.awai.cc/v1`。
- 默认模型为 `gpt-5.6-sol`；模型列表通过用户已配置的 API Key 请求 `/models`，
  获取失败时仍允许手动输入模型名。
- API Key 只显示“已配置/未配置”，写入 Codex 的本机凭据文件，永不写入 `config.toml`
  的诊断摘要、皮肤包、Git 或远程日志。
- `disable_response_storage`、`goal_mode`、`personality`、`approval_policy` 和
  `sandbox_mode` 写入 Codex 支持的 TOML 字段，并在保存后重新解析验证。
- `approval_policy = "never"` 与 `sandbox_mode = "danger-full-access"` 会增加风险，
  界面必须显示明确警示；默认值不能偷偷切到危险模式。

## 皮肤原则

皮肤是素材和样式包，不是配置包。AWAI 皮肤库由独立的 GitHub/Gitee 仓库维护；每个包
必须声明版本、预览图、作者、许可证和 SHA-256。品牌文案、API 地址和 API Key 不进入
`.codexskin`，这样主题可以分享而不会改变用户的服务端配置。

应用皮肤时使用 Codex 支持的运行时注入和配置接口，失败就恢复上一个快照。任何皮肤操作
都不得改写、重新签名或删除 Codex 安装文件。

## 信任与失败体验

信任顺序是：HTTPS 传输 → manifest/版本字段 → SHA-256 → Windows Authenticode 或
macOS Developer ID/Sparkle EdDSA → 安装后健康检查。任一环节失败都停止在暂存区，保留
诊断信息，不触碰当前可运行版本。

镜像、GitHub 或自定义源只是传输渠道，不是信任替代物。Manager 不会因网络失败而跳过
校验，也不会把失败包装成“已完成”。

## 当前范围与明确不做

- 当前支持 Windows x64/ARM64 和 macOS Apple Silicon/Intel。
- 当前只支持 stable 渠道；manifest 预留 channel 字段供以后扩展。
- 不做账户系统、遥测、广告、对话同步、工作区上传或破解官方授权。
- 卸载 Manager 或 Codex 时默认保留用户数据；清除数据必须单独确认。

架构细节见 [`architecture.md`](./architecture.md)，后续工作见 [`roadmap.md`](./roadmap.md)。
