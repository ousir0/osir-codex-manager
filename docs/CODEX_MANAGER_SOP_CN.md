# Codex Manager 标准作业流程（SOP）

版本：1.0
适用仓库：`ousir0/osir-codex-manager`
适用范围：Manager 代码修改、Codex/OpenCodex 配置管理、版本升级、跨平台发版、线上镜像和用户侧一键更新。

本文是日常执行入口。更详细的签名、平台构建和回滚约束分别参见：

- [`CLIENT_RELEASE_RUNBOOK_CN.md`](./CLIENT_RELEASE_RUNBOOK_CN.md)
- [`release.md`](./release.md)
- [`code-signing-policy.md`](./code-signing-policy.md)
- [`windows-signing.md`](./windows-signing.md)

## 1. 总原则

1. Manager 是配置和发布控制层；OpenCodex 是可选的多模型路由组件；Codex 本体仍读取自己的配置和会话数据。
2. 代码、配置管理或 OpenCodex 集成发生可感知变化时，必须递增 Manager 版本，不能覆盖已发布版本。
3. GitHub Immutable Release 是发布工件的唯一来源；线上镜像只能复制并校验同一批字节。
4. 用户侧只通过 Manager 内置更新器升级：发现更新 → 用户确认 → 签名校验 → 下载安装 → 自动重启。
5. 任何验证或上传失败都不能推进线上 `latest.json`，必须保留上一版本可回滚。
6. 真实用户配置、API Key、证书、私钥和临时下载目录不得进入 Git。

## 2. 需求和问题处理流程

### 2.1 先判断责任边界

- 接口地址、配置写入、供应商列表、OpenCodex 路由、会话索引迁移：归 Manager。
- 多模型服务进程、路由实际转发、供应商授权结果：归 OpenCodex 或上游服务。
- Codex 原生登录、原生会话正文和 `CODEX_HOME`：归 Codex 本体。

现象不能直接当成根因。例如“会话消失”优先检查 `state_5.sqlite` 的 `threads.model_provider/model` 是否仍绑定旧路由，而不是删除会话文件。

### 2.2 固定诊断顺序

1. 记录错误原文、当前配置模式、目标模式、当前模型和网关地址。
2. 检查 Codex 配置、OpenCodex 配置、Manager 状态、模型目录和会话索引是否互相一致。
3. 区分确定性错误（401/403、配置格式错误、模型不存在）和临时错误（429、502/503/504、超时、连接重置）。
4. 先做只读验证，再修改配置；修改前创建可恢复备份。
5. 修复后必须验证：当前配置生效、模型可调用、旧会话可见且可以继续对话。

## 3. 配置切换标准

### 3.1 默认配置与 OpenCodex 多模型

- UI 中的“默认配置”代表用户现有 Codex `config.toml`，不是单一模型。
- 默认配置可以包含多个供应商；供应商列表展示在左侧，右侧展示当前供应商、模型、网关和连接状态。
- OpenCodex 多模型模式负责统一呈现所有已保存供应商的模型。新增供应商后，必须同步到 Codex 选择器。
- 点击列表只改变查看对象；只有点击“启用”并确认后才改变实际生效配置。

### 3.2 启用前检查

1. 弹窗明确目标模式、目标供应商/模型、是否需要重启。
2. 默认配置：使用真实 `/responses` 最小请求验证目标模型。内置 OpenAI/ChatGPT 登录且无自定义 Base URL 时交由 Codex 自身认证。
3. OpenCodex：先确保服务 ready、模型目录完整，再检测默认路由。
4. 401/403 等确定性鉴权错误立即阻止切换，并保持当前可用模式。临时上游错误按退避策略重试。
5. 所有配置和状态写入必须原子化；中途失败自动恢复切换前文件。

### 3.3 会话连续性

- 切换时只事务性更新 `state_5.sqlite` 的 `threads.model_provider` 和 `threads.model`。
- 不移动、不删除、不重写 JSONL 会话正文。
- OpenCodex 路由模型（如 `provider/model`）切回默认配置时转换为裸模型；默认配置切回 OpenCodex 时恢复到已保存路由。
- 不修改无法确认归属的第三方 Provider 或未知模型。
- 迁移前生成 `codex-session-index.before-switch.sqlite` 一致性备份。
- 旧版本用户打开配置管理时允许自动修复遗留索引；若 Codex 正在运行，必须标记“需要重启”。

### 3.4 切换后的用户提示

- 配置已写入但 Codex 正在运行：提示“重启 Codex 即可生效”，提供明确的重启入口。
- 连接测试失败但真实调用成功：以真实模型调用结果为准，不能只依赖 `/models`。
- 切换失败：明确说明原因、保持当前模式，并提供修复或恢复备份动作。

## 4. 开发修改流程

1. 在开始前读取本 SOP、相关模块文档和当前 `git status`。
2. 先写或更新测试，再修改实现；涉及配置切换必须覆盖成功、鉴权失败、临时失败重试和回滚。
3. 前端交互变更要覆盖：列表查看不切换、启用确认、错误保持当前状态、重启提示。
4. 后端变更要覆盖：原子写入、会话索引迁移、备份、未知 Provider 保留。
5. 只修改任务范围内文件；不删除或提交用户已有未跟踪文件。
6. 完成后更新对应 `docs/releases/vX.Y.Z.md`，只写已经验证的行为。

## 5. 本地质量门

在提交前执行：

```bash
npm run check
npm test -- --run
npm run test:release
cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked
npm run build
git diff --check
```

需要打本地 macOS 包时执行：

```bash
npm run tauri:build:mac
bash scripts/finalize-macos.sh
codesign --verify --deep --strict "src-tauri/target/release/bundle/macos/Codex Manager.app"
```

本地包用于验收；Windows x64/ARM64 和 macOS Intel/ARM64 的干净环境结果以 GitHub Actions 为准。

## 6. 版本和提交规范

### 6.1 版本升级

目标版本必须是比线上版本更高的新版本，例如 `0.5.29`：

```bash
npm version 0.5.29 --no-git-tag-version
cargo update -p osir-codex-manager --manifest-path src-tauri/Cargo.toml --offline
node scripts/check-release-version.mjs source v0.5.29 .
```

必须一致的位置：`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。

### 6.2 提交前检查

```bash
git status --short --branch
git diff --check
git diff --stat
```

提交只包含本次任务文件：

```bash
git add <本次修改文件>
git commit -m "fix: <简短变更说明>"
git push origin main
```

工作区“干净”的定义是：本次修改已提交；用户原有未跟踪文件可以保留，但必须在交付记录中列出，不能误删或声称绝对干净。

## 7. 正式发版流程

1. 只对已推送到 `main` 的完整 commit 创建带注释标签：

   ```bash
   git tag -a v0.5.29 -m "Codex Manager v0.5.29" <完整SHA>
   git push origin v0.5.29
   ```

2. 监控 `CI`、`Release source` 和 `Release` 工作流；失败时先判断是否为临时容量/网络问题，确认没有产生 Release 或线上变更后再重试。
3. Release 必须是 `isDraft=false`、`isImmutable=true`，且包含 Windows x64/ARM64 安装包及签名、macOS Intel/Apple Silicon DMG/tar.gz 及签名、`latest.json`、`SHA256SUMS`、`release-binding.json`、SBOM 和状态文件。
4. 不把 macOS 本地未公证包当作正式分发包；正式签名、公证和 Windows Authenticode 以受保护 CI 结果为准。

## 8. 线上镜像和用户更新

### 8.1 镜像

每次远端操作前固定执行：

```bash
cd /Users/ouwei/sub2api-new
tools/audit_rainyun_connection_targets.sh
cd /Users/ouwei/codex-app-manager
npm run publish:manager -- v0.5.29
```

固定目标为 `root@100.82.197.6`，通过 Tailscale 审计；禁止临时公网 IP、未审计 SSH 别名或其他服务器。发布脚本必须：下载 GitHub 不可变 Release、逐文件 SHA-256 校验、上传版本目录、校验完整性后原子切换 `current`。

### 8.2 线上回读

```bash
curl -fsSL https://app.osirclaw.com/manager/latest.json | jq -r '.version, (.platforms | keys[])'
for name in CodexManager_0.5.29_x64-setup.exe CodexManager_0.5.29_arm64-setup.exe CodexManager_aarch64.dmg CodexManager_x86_64.dmg; do
  curl -L -sS -o /dev/null -w "$name %{http_code} %{size_download}\n" "https://app.osirclaw.com/manager/latest/$name"
done
```

必须确认版本是目标版本、四个平台齐全、四个下载入口返回 200，并且线上字节哈希与 GitHub Release 一致。

### 8.3 用户侧一键更新

用户不需要手工替换应用或重新安装开发包，操作只有：

1. 打开旧版 Manager，进入更新检查入口或等待检查；
2. 看到新版本说明后点击“立即更新”；
3. 客户端重新获取清单并校验版本、URL 和 updater 签名；
4. 下载、安装并自动重启；
5. 重启后在“关于”页确认版本等于目标版本。

如果更新失败，必须保留旧客户端可启动；不要重复覆盖同一个已发布版本。

## 9. 回滚流程

### 9.1 配置切换回滚

- 使用切换前 `config.toml`、Manager 状态、OpenCodex 配置和会话索引备份。
- 不删除会话正文；先恢复索引，再重启 Codex。

### 9.2 线上发布回滚

1. 先只读确认 `current`、版本目录和上一稳定版本。
2. 不删除错误版本，不覆盖历史对象。
3. 将 `current` 原子切回已验证旧版本。
4. 回读 `latest.json` 和四个平台 URL。
5. 修复必须使用更高的新版本号。

## 10. 发布记录模板

发布完成后可直接生成验收报告（默认写入 `docs/release-reports/vX.Y.Z.md`）：

```bash
npm run release:report -- vX.Y.Z
```

若需要在离线或 CI 复核环境运行，可注入已保存的 Release 和 `latest.json`：

```bash
node scripts/release-report.mjs vX.Y.Z \\
  --release-json /path/to/release.json \\
  --latest-json /path/to/latest.json \\
  --output docs/release-reports/vX.Y.Z.md
```

报告会记录 Git SHA、工作区状态、Release 资产、四平台线上 HTTP/大小、`latest.json`、CI/镜像状态和用户更新验收项；URL 查询参数会被清理，不记录预签名链接或凭据。

每次发版在 PR、Release 或交付记录中保留：

```text
版本：vX.Y.Z
源码 SHA：<full commit SHA>
CI Run：<run id>
四平台构建：Windows x64 / Windows ARM64 / macOS Intel / macOS ARM64
Release：isDraft / isImmutable / 资产清单
线上目录：/var/www/osir-codex-manager/releases/X.Y.Z
current：<真实目录，不是未解析的符号链接>
latest.json：<线上版本和平台键>
下载回读：四个平台 HTTP 状态、大小、SHA-256
客户端验收：发现更新 / 点击更新 / 重启 / 版本确认
异常与回滚：<无或记录原因、旧版本和恢复动作>
工作区遗留：<用户未跟踪文件，若有>
```

## 11. 已验证基线与待建设项

截至 `v0.5.29`，以下能力已验证：默认配置与 OpenCodex 双向切换、真实网关探测和临时错误重试、会话索引双向迁移和一致性备份、前端启用确认/失败保持当前模式/重启提示、前端 353 项/发布流程 59 项/Rust 210 项测试、四平台 GitHub Release/线上镜像/`latest.json`、macOS 本地包生成/签名/DMG 内应用校验。

仍需单独建设并验收的增强项：Manager 启动后全局自动弹出更新提示、更新重启后的统一成功确认界面、自动生成发布报告并接入 CI、真实旧版客户端端到端点击更新的自动化冒烟。

## 反思

这套 SOP 固化的是“可验证、可回滚、可追溯”的交付链路；最大风险不再是单次代码修改，而是跳过线上回读或把本地构建误当成正式分发包。
