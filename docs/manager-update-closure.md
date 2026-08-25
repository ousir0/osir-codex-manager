# Codex Manager 自更新闭环

评估日期：2026 年 8 月 20 日。

> 发布约定：后续 Manager 代码或 OpenCodex 集成修复必须递增 Manager 版本并走版本化 Release。线上 `latest.json` 推进后，用户只需在客户端点击“立即更新”；不再要求用户手动替换应用或重新安装开发包。

## 结论

当前项目已经具备 Manager 自更新的核心基础设施，但还没有完全闭环。

已完成：

- 客户端内置 Tauri updater 公钥和更新地址；
- 云端有版本化安装包、latest.json 和 updater 签名；
- 发布工作流负责构建、签名、生成清单、上传和公开回读校验；
- “关于”页可以手动检查并安装 Manager 更新；
- 当前主更新地址是 https://app.osirclaw.com/manager/latest.json；GitHub 正式发布已到 v0.5.6。

未完成：

- 用户启动 Manager 后不会自动检查 Manager 更新并弹窗；
- 设置里的“启动时检查 / 定时检查”目前主要检查 Codex 客户端，不是 Manager 自身；
- 更新重启后没有统一的“已升级到目标版本”确认界面。

因此当前状态是：云端发布链路基本具备，用户自动提醒链路未完成。

## 两条更新线

| 更新对象 | 更新入口 | 版本来源 |
| --- | --- | --- |
| Codex 客户端 | 首页更新卡片 | macOS appcast / Windows manifest |
| Codex Manager | 关于页手动检查 | Tauri /manager/latest.json |

本地修改 src/、src-tauri/、配置页或 OpenCodex 逻辑，必须发布 Manager 新版本，不能只更新 Codex 客户端清单。

## 目标闭环

本地修改代码 → 更新 Manager 版本号 → 提交默认分支并通过 CI → 创建 vX.Y.Z tag → GitHub Actions 构建四个平台 → 生成 updater 签名和 latest.json → 上传版本化对象 → 公开回读校验 → 推进 app.osirclaw.com/manager/latest.json → 用户启动或定时检查 → 弹窗显示新版本 → 用户确认下载、安装、重启 → 重启后确认版本已更新。

## 当前代码已经做好什么

### 客户端信任链

配置位于 src-tauri/tauri.conf.json：

- endpoint：https://app.osirclaw.com/manager/latest.json；
- 内置 Tauri updater 公钥；
- 安装前校验 updater 签名；
- 安装前再次检查目标版本和当前版本，防止清单变化后误装。

### 云端版本管理

当前 GitHub 正式发布 v0.5.6 包含 macOS arm64、macOS Intel、Windows x64、Windows ARM64。安装包位于按版本隔离的路径，例如 /manager/0.5.6/...；latest.json 位于固定根地址，客户端只需要轮询一个 URL。网站镜像 promotion 仍需配置对象存储凭据。

旧文档中的 codexapp.awai.cc 不是当前客户端配置的主地址，日常发布应以 app.osirclaw.com 为准。

### 发布工作流

.github/workflows/release.yml 已包含：

1. 四个平台构建；
2. 平台架构检查和测试；
3. updater 工件签名；
4. 生成 latest.json；
5. GitHub Release 上传；
6. 对象存储 / CDN 镜像同步；
7. 对公开 URL 做长度、SHA-256、签名和清单回读校验；
8. 校验通过后才推进 latest.json。

版本对象按设计不可覆盖，修复必须发布更高版本。

## 当前真正缺少什么

### 1. 自动弹窗入口缺失

前端已有 managerApi.checkManagerUpdate()，About.tsx 也能手动调用它。

但首页启动检查和定时检查调用的是：

- macOS：macPlanUpdate()；
- Windows：winPlanUpdate()。

这两者检查的是 Codex 客户端，不是 Manager 自身。因此用户即使打开“启动时检查更新”，也不代表 Manager 会弹窗。

### 2. 更新状态只存在于关于页

当前待更新状态主要由 About.tsx 持有。用户不进入关于页，就看不到 Manager 更新。

要闭环，需要增加应用级更新状态：启动后后台检查一次、按设置定时检查、多页面共享待更新状态、任意页面显示轻量提示、点击后打开统一确认弹窗，并在更新期间锁定导航。

### 3. 更新后缺少确认

安装并重启后，应重新读取当前 Manager 版本并确认等于目标版本。若仍是旧版本，应显示“更新未生效”，不能只显示安装成功。

## 本地开发如何更新用户侧

### 只改本地代码

修改代码 → 本机开发运行 / 本机打包。

只有本机生效，已安装用户不会变化。

### 不改版本号重复打包

当前 0.5.6 → 修改代码 → 仍构建 0.5.6。

不应这样发布。更新器会把它视为同一版本，云端版本对象也按不可覆盖设计。

### 正确做法

修改代码 → 版本号改为 0.5.6 → 本地检查和打包 → 提交默认分支 → 创建 v0.5.6 tag → GitHub Actions 构建、签名、发布 → app.osirclaw.com 推进 latest.json → 用户侧检查到 0.5.6 → 用户确认安装并重启。

这是用户侧真正能收到更新的可靠路径。

## 每次发布必须同步的版本位置

以 v0.5.6 为例，以下版本必须一致：

- package.json；
- package-lock.json 根版本；
- src-tauri/tauri.conf.json；
- src-tauri/Cargo.toml；
- src-tauri/Cargo.lock 中 osir-codex-manager 的版本。

本地检查：

    npm ci
    npm run check
    npm test
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    git diff --check
    node scripts/check-release-version.mjs source v0.5.6 .

## 正式发布 SOP

### 版本准备

1. 决定新版本，例如 0.5.6；
2. 同步五处版本号；
3. 写 docs/releases/v0.5.6.md；
4. 执行版本一致性检查；
5. 提交默认分支。

### 创建发布

    git tag v0.5.6
    git push origin v0.5.6

之后由 GitHub Actions 完成构建、签名、上传、镜像同步和清单推进。

### 发布后检查

    curl -fsSL https://app.osirclaw.com/manager/latest.json | jq .
    curl -I https://app.osirclaw.com/manager/0.5.6/CodexManager_aarch64.app.tar.gz
    curl -I https://app.osirclaw.com/manager/0.5.6/CodexManager_0.5.6_x64-setup.exe

然后用低版本 Manager 做真实测试：启动旧版本、等待自动检查、确认弹窗显示新版本、点击更新、等待重启、在关于页确认版本已经变成目标版本。

在自动弹窗功能完成前，只能通过关于页手动触发检查。

## 自动弹窗的推荐实现

### 启动检查

Manager 启动、操作恢复完成、主窗口可用后，后台调用 managerApi.checkManagerUpdate()。只检查，不自动安装。发现更新后写入应用级状态，再显示非阻塞提示和确认弹窗。

### 定时检查

沿用现有定时检查设置，但它应同时覆盖 Codex 客户端更新和 Manager 自更新。网络失败只显示不可用或记录日志，不打断正常使用。

### 弹窗最少显示

- 当前版本；
- 最新版本；
- 版本说明；
- 立即更新；
- 稍后提醒；
- 跳过此版本。

跳过应按具体版本保存，不能永久关闭所有提醒。

### 安装和重启

1. 用户确认后再次请求清单；
2. 校验目标版本没有变化；
3. 下载并验证 updater 签名；
4. 安装更新；
5. 重启 Manager；
6. 启动后重新读取当前版本；
7. 显示成功或失败结果。

后端已经具备前 4 步的核心保护，当前缺的是全局入口和重启后的确认展示。

## 最终验收标准

只有同时满足以下条件，才能说 Manager 自更新完全闭环：

- 新代码能生成新版本工件；
- 所有版本号一致；
- GitHub Release 和版本化对象存在；
- latest.json 已推进到新版本；
- 四个平台条目齐全或明确 partial；
- updater 签名校验通过；
- 旧版客户端启动后自动检查；
- 用户能看到更新弹窗；
- 用户确认后能下载、安装和重启；
- 重启后版本变成目标版本；
- 更新失败时旧版本仍可运行；
- 跳过旧版本后，更高版本仍会提醒。

当前项目已完成发布和签名链路的大部分，尚未完成“旧版客户端自动弹窗 → 更新后确认”的用户体验闭环。

## 相关代码

- updater 配置：src-tauri/tauri.conf.json
- 后端检查与安装：src-tauri/src/commands.rs
- 前端 updater API：src/services/managerApi.ts
- 当前手动入口：src/app/views/About.tsx
- Codex 客户端更新：首页 src/app/views/Home.tsx、src/app/views/WinHome.tsx
- 发布工作流：.github/workflows/release.yml
- 清单生成：scripts/gen-updater-manifest.mjs
- 发布验签：scripts/verify-release-artifacts.mjs
- 镜像同步：scripts/sync-mirror.sh、scripts/mirror-release.mjs
- CDN 路由：cloudflare/manager-download-router/
