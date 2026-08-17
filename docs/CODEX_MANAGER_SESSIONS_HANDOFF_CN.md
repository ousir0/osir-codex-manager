# OSIR Codex Manager 会话交接与落地入口

整理日期：2026-08-17

## 来源会话

- 当前落地会话：`01a0109f-8289-7013-af31-3371764c84e4`（整理会话资料并落地项目）
- 前置会话：`01a00ee9-ac4e-7d93-a529-9de46b8784c6`（拉取项目并准备重新打包）

## 结论

目标不是给原客户端换 Logo，而是把它迁移成由 OSIR 独立控制的产品：源码仓库、产品身份、运行时服务、更新信任链、安装包镜像、皮肤、签名、发布流水线和监控都不能继续依赖原作者的账号或基础设施。

当前仓库负责 **OSIR Codex Manager 客户端**。`/Users/ouwei/sub2api-new` 负责 **OSIR API 控制面**，包括用户、API Key、模型目录、订阅、额度、路由、计费和审计。大型安装包分发、Manager 自更新、皮肤目录和国际化隧道应保持独立部署边界。

## 已确认方向

- 产品名：`OSIR Codex Manager`。
- 主二进制名：`osir-codex-manager`。
- macOS Bundle ID：`com.osir.codexmanager`。
- Git 主仓库由 `ousir0` 控制；原仓库只保留为不可推送的 `upstream`。
- 客户端 Provider ID 使用 `osir`，展示名统一为 `OSIR`。
- OSIRAPI 生产入口暂按 `https://api.osirclaw.com/v1` 进入接入合同，发布前仍需做域名所有权和可用性确认。
- Manager 更新必须使用 OSIR 新生成的 updater 密钥对。
- Windows/macOS 正式版本最终必须使用 OSIR 自己的签名身份。

## 当前仓库基线

- 分支：`dev/ouwei-local`。
- 冻结来源提交：`23203702c55c8b2476df5fad5c3d42af56fe0d85`（`v0.5.3`）。
- `origin`：`ousir0/codex-app-manager`。
- `upstream`：原仓库，只读，push 已禁用。
- 已有未提交修改：Cargo 作者/仓库、Tauri 包描述、关于页仓库链接。
- 已有评估文档：
  - [完整独立化评估](./osir-full-ownership-assessment.md)
  - [原服务与云端位置清单](./service-inventory.md)

风险：当前工作区不是可发布来源。未提交修改需要先审查、测试并形成明确提交，不能直接从脏工作区打正式包。

## 项目边界

| 能力 | 主责任系统 |
|---|---|
| 客户端品牌、安装、配置、主题、本地数据迁移 | 当前仓库 |
| 用户、API Key、模型、额度、计费、路由、审计 | `sub2api-new` |
| Manager updater、签名、版本工件 | OSIR CI + 对象存储 |
| Codex Windows/macOS 包采集与镜像 | 独立镜像服务 |
| 在线皮肤目录 | 独立 OSIR 皮肤仓库/对象存储 |
| 国际化 WebSocket 隧道 | 独立受限 relay 服务 |

## 第一项落地工作

阶段 0 先建立可验证基线：

1. 用根目录 `SPEC.md` 固定目标、非目标、实施顺序和验收方式。
2. 用 `npm run audit:ownership` 持续统计旧品牌、账号、域名、Bundle ID 和外部镜像引用。
3. 第一阶段只生成报告，不让现有 CI 立即失败；当生产路径清零后切换到 `npm run audit:ownership:strict`。
4. 不在本阶段修改生产域名、上传密钥、发布安装包或部署服务。

这一步的影响是把“约 118 个文件”从一次性人工判断，变成每次改动都能重复验证的迁移指标。

首次审计基线：扫描 295 个文本文件，60 个文件存在 254 行旧归属引用；其中运行路径为 27 个文件 / 124 行。后续迁移以该命令的动态结果为准，不再依赖人工估算。
