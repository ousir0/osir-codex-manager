# Codex Manager 客户端发布与自动更新标准化任务

状态：已沉淀为长期执行标准；已落地部分见 [`CODEX_MANAGER_SOP_CN.md`](./CODEX_MANAGER_SOP_CN.md)。

## 目标

把“修改代码 → 测试 → 构建 → 发布 → 镜像 → 用户点击更新”固化成一条可重复执行的流水线，减少人工操作、等待时间和重复排查。

最终要求：

- Windows x64/ARM64、macOS Apple Silicon/Intel 自动构建并验证。
- GitHub Release、Rainyun 镜像和 latest.json 使用同一批文件。
- 用户可以在客户端点击“立即更新”完成升级。
- 任一步失败都不推进线上 latest，并保留旧版本回滚。

## 本次暴露的问题

1. GitHub Release 成功后，Rainyun Manager 镜像没有自动同步，线上 latest.json 仍是旧版本。
2. 手工复制 current 时曾保留旧符号链接，候选 release 不是独立真实目录。
3. 大文件下载可能中断，必须做 SHA-256 校验和失败重试。
4. Windows 干净环境验证必须依赖 Windows CI，不能用 macOS 本地测试代替。
5. Rainyun 生产服务已经演进为持久化蓝绿服务，不能固定假设旧的 preview service 和端口。

## 标准流程

### 1. 发布前冻结

- 确认目标版本号。
- 检查 package、Tauri、Cargo 版本一致。
- 确认工作区干净并记录完整 commit SHA。
- 生成 release note。
- 审查未合并分支、worktree 和孤儿提交。

### 2. 质量门

    npm run check
    npm test
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    git diff --check

Windows/macOS 安装包验证以 CI 结果为准。

### 3. 干净环境验证

必须通过：

- Windows x64：安装、首次启动、OpenCodex 组件安装、升级、卸载。
- Windows ARM64：构建、PE 架构和签名资产检查。
- macOS Apple Silicon/Intel：构建、启动和更新流程。
- OpenCodex clean-component smoke：组件哈希、内置 Node、版本和配置校验。

### 4. 正式发布

- 只从合并到 main 的 commit 打 tag。
- 生成四个平台安装包、签名文件、latest.json、SHA256SUMS 和 release binding。
- 先验证资产完整性，再发布 GitHub Release。
- macOS 公证、Windows Authenticode 必须据实标注，不能与 Tauri updater 签名混淆。

### 5. Rainyun 镜像

发布流水线应自动完成：

1. 下载 GitHub Release canonical assets。
2. 校验每个文件 SHA-256。
3. 创建真实版本目录，例如 manager/0.5.11/。
4. 生成指向版本目录的 manager/latest.json。
5. 更新 manager/latest/ 稳定下载入口。
6. 临时目录上传成功后原子切换 current。
7. 回读四个平台 URL、状态码、大小、ETag 和 SHA-256。

禁止直接复制旧 current 符号链接；必须解析后复制真实目录。

### 6. 用户更新验收

发布后必须确认：

- latest.json.version 等于目标版本。
- 清单包含四个平台。
- URL 全部指向目标版本目录。
- 资产返回 200，大小和 SHA-256 与 GitHub Release 一致。
- 客户端更新按钮能完成下载、签名校验、安装和重启。

### 7. 回滚

- 至少保留一个上一版本目录。
- 更新 latest 前保留旧文件和旧 current 链接。
- 镜像异常时恢复旧 current，不删除旧版本。
- 客户端安装失败时保留原客户端可启动。
- 后端和客户端发布可以分别回滚。

## 后续标准化建设

### 已完成：统一发布入口

当前已具备受保护 GitHub Action 和 `npm run publish:manager -- vX.Y.Z` 统一入口：

    版本检查
    → 本地/CI 测试
    → 四平台构建
    → GitHub Release
    → Rainyun Manager 镜像
    → latest.json 回读
    → 用户更新验收报告

### 已完成：幂等镜像脚本

要求：不覆盖历史版本、上传失败清理临时目录、校验失败禁止切换、latest 与版本目录原子更新，并固定使用 Rainyun Tailscale 目标。

### P1：自动更新 smoke

模拟当前版本低于线上版本，验证更新提示、下载 URL、签名校验、安装入口和失败保护。

### P1：发布报告

自动记录版本、完整 SHA、四平台资产大小和哈希、CI 结果、Rainyun 目录、current 指向、latest 回读结果和回滚路径。

## 与密钥托管改造的顺序

必须先完成本发布标准化，再进行 OpenCodex 专用托管 Key 改造。否则客户端、OSIRAPI 后端、密钥生命周期和用户升级问题会混在一起，难以定位。

## 完成标准

- [x] 一条命令或一个受保护 Action 完成客户端发布。
- [x] Windows/macOS 干净环境验证自动执行。
- [x] GitHub Release 与 Rainyun 资产 SHA-256 一致。
- [x] latest.json 自动推进且包含四个平台。
- [x] 用户点击更新按钮可完成升级。
- [x] 发布失败不会推进 latest。
- [ ] 自动生成发布报告和回滚信息。

当前 P1 两项仍是后续增强，不影响现有版本化发布和用户点击更新流程。

## 反思

这里解决的是“发布过程不稳定”的根因，而不是继续依赖人工补救单次发布；最大假设是 GitHub Release 继续作为 canonical asset 来源。
