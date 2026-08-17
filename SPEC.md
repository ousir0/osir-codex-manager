# OSIR Codex Manager 独立化实施规格（草案）

状态：阶段 0、阶段 1 已完成；阶段 2/3 执行中，长期 `GOAL.md` 已编译。  
来源会话：`01a0109f-8289-7013-af31-3371764c84e4`、`01a00ee9-ac4e-7d93-a529-9de46b8784c6`。

## 目标

将当前客户端迁移为 `OSIR Codex Manager`，使产品身份、运行时服务、构建发布、更新信任链和用户可见入口由 OSIR 独立控制，并保留现有安装、更新、配置、主题和卸载能力。

## 非目标

- 不在客户端仓库内实现 OSIRAPI 用户、计费和路由后端。
- 不在第一阶段部署生产域名、对象存储、镜像服务或国际化隧道。
- 不复用原作者的 updater 私钥、云密钥或签名证书。
- 不通过无差别字符串替换破坏历史迁移逻辑、测试夹具和兼容路径。

## 已确认产品身份

| 字段 | 目标值 |
|---|---|
| 产品名 | `OSIR Codex Manager` |
| 主二进制名 | `osir-codex-manager` |
| macOS Bundle ID | `com.osir.codexmanager` |
| Provider ID | `osir` |
| Provider Name | `OSIR` |
| GitHub 所有者 | `ousir0`（后续可迁入 OSIR Organization） |
| API Base URL | `https://api.osirclaw.com/v1`（发布前验证） |

## 基线

- 来源版本：`v0.5.3`。
- 来源提交：`23203702c55c8b2476df5fad5c3d42af56fe0d85`。
- 开发分支：`dev/ouwei-local`。
- 原仓库只读远程：`upstream`，push 已禁用。
- 当前工作区已有会话遗留修改，必须保留并审查。

## 架构边界

- 当前仓库：桌面客户端、安装器、本地配置、主题和升级兼容。
- `sub2api-new`：用户、API Key、模型、额度、路由、计费和审计。
- 独立服务：Manager updater、Codex 包镜像、皮肤目录、国际化 relay、官网与状态页。

## 实施阶段

### 阶段 0：冻结与审计

- 归档两次会话的结论和系统边界。
- 记录 Git 基线和未提交状态。
- 建立旧归属引用审计命令。
- 不修改生产服务、不发布、不写入密钥。

### 阶段 1：产品身份与本地数据兼容

- 修改产品名、二进制名、Bundle ID、安装器、菜单、日志和多语言。
- 将旧数据目录一次性迁移到 OSIR 数据目录；迁移必须幂等，且不能覆盖较新的 OSIR 数据。
- 将 `awai` Provider 配置迁移为 `osir`，但不能丢失用户 Key。
- 更新测试和打包冒烟脚本。

### 阶段 2：独立构建与更新信任链

- 将 Release、工件名、Winget、签名和发布审批迁入 OSIR。
- 生成新的 Tauri updater 密钥对；私钥只进入受保护发布环境和离线备份。
- 产出未发布的 Windows/macOS 测试包并完成新装验证。

### 阶段 3：OSIR 运行时服务

- 接入 OSIR Manager updater 和对象存储。
- 接入 OSIR Codex 包镜像与校验清单。
- 接入 OSIRAPI、皮肤目录和受限国际化 relay。
- 保留明确、可观测的官方源或本地导入回退策略。

### 阶段 4：正式发布

- 完成 Windows/macOS 签名、公证、新装、升级、回滚和卸载验收。
- 使用网络审计确认生产客户端不再访问旧服务。
- 发布官网、下载页、隐私说明、状态页和版本校验信息。

## 阶段 0 验收标准

- [x] 项目内存在两次会话的交接文档。
- [x] 项目内存在 API/客户端边界文档。
- [x] 记录来源提交、分支、远程仓库和未提交风险。
- [x] `npm run audit:ownership` 可输出旧归属引用的文件数、行数和分类。
- [x] `npm run check`、`npm run build`、`npm test`、Rust 单元测试通过。

首次审计基线：295 个文本文件中，60 个文件存在 254 行匹配；运行路径为 27 个文件 / 124 行。

已知基线问题：全仓 `npm run lint` 因 [WinHome.tsx](./src/app/views/WinHome.tsx) 的 `hostArchitecture` 参数未使用而失败。该问题早于本次阶段 0 改动，本阶段未修改业务文件。

阶段 1 验收已按下方 `done_when` 执行；发布状态见 `docs/OSIR_RELEASE_HANDOFF_20260817_CN.md`。

## 阶段 1 候选 done_when（待用户确认）

1. `src-tauri/tauri.conf.json` 的产品名、主二进制名和 Bundle ID 均为 OSIR 值。
2. 安装器、系统菜单、关于页、诊断标题和多语言界面显示 `OSIR Codex Manager`。
3. 旧数据目录迁移测试覆盖：首次迁移、重复启动、目标已存在、源损坏四种情况。
4. Provider 迁移测试证明旧 API Key 不丢失且新配置使用 `osir`。
5. `npm run audit:ownership:strict` 对生产运行路径不再报告旧品牌、账号、域名和 Bundle ID；历史文档与专用迁移夹具允许保留。
6. `npm run check`、`npm run lint`、`npm test`、`cargo test --manifest-path src-tauri/Cargo.toml --lib` 全部通过。
7. macOS 本地开发包能启动；Windows x64 由 GitHub Actions 产出测试安装包。

## 风险边界

- 修改 Bundle ID 会创建新的系统应用身份；没有迁移逻辑会导致设置、主题和自动启动项看似丢失。
- updater 地址和公钥必须成套迁移；任何一项提前切换都会让客户端更新失败。
- 当前生产域名、签名身份和云资源仍待实际所有权验证，不能只凭文档值发布。
- 当前工作区有未提交改动；正式构建必须来自审查后的明确提交 SHA。

## 验证命令

```bash
npm run audit:ownership
npm run check
npm run lint
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
git diff --check
```

## 待确认决策

唯一会显著改变阶段 1 实现的决策：旧版用户数据是自动迁移到 OSIR 新目录，还是将 OSIR 版视为完全全新安装。本文档默认采用自动、幂等迁移，因为用户损失风险更低。
