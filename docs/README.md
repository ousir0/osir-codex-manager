 # 项目文档索引 (Documentation Index)
 
 本项目（Codex Manager / OSIR Codex Manager）的文档按场景与层级分类如下：
 
 ---
 
 ## 1. 用户与产品文档 (User & Product)
 
 - [用户手册 (中文)](user-guide-zh-CN.md) - 面向终端用户的快速上手与配置指南。
 - [User Guide (English)](user-guide-en.md) - English guide for installation, update, and configuration.
 - [功能列表与规划 (Features)](features.md) - 核心功能清单与产品能力概览。
 - [产品设计说明 (Product Design)](product-design.md) - 交互设计与产品设计原则。
 - [产品与官网规划 (Product & Website Plan)](product-and-website-plan.md) - 官网构建与产品演进规划。
 - [隐私政策 (Privacy Policy)](privacy.md) - 本地数据处理与网络请求隐私合规政策。
 - [语言切换规范 (Language Switching)](language-switching.md) - 中英文国际化切换策略。
 
 ---
 
 ## 2. 架构与核心规范 (Architecture & Specs)
 
 - [系统架构 (Architecture)](architecture.md) - 前端 (React/Vite) 与宿主 (Tauri v2 / Rust) 架构设计。
 - [服务资产清单 (Service Inventory)](service-inventory.md) - 项目内各模块、服务与依赖盘点。
 - [Manifest 契约 (Manifest Contract)](manifest-contract.md) - 多平台版本分发清单格式与校验规则。
 - [Webview 右键菜单规范 (Webview Context Menu)](webview-context-menu.md) - 跨平台 Webview 交互规范。
 - [主题适配审计 (Theme Adaptation Audit)](codex-theme-adaptation-audit.md) - Codex 官方界面注入与主题适配分析。
 
 ---
 
 ## 3. 发版与签名 (Release & Code Signing)
 
 - [客户端发版操作手册 (SOP)](CLIENT_RELEASE_RUNBOOK_CN.md) - 标准发版流程、环境变量配置与发布步骤。
 - [标准发版计划 (Release Standardization Plan)](CLIENT_RELEASE_UPDATE_STANDARDIZATION_PLAN_CN.md) - 发版与更新标准化方案。
 - [通用发布文档 (Release Guide)](release.md) - 基础 Release 构建与说明。
 - [代码签名政策 (Code Signing Policy)](code-signing-policy.md) - macOS / Windows 代码签名与公证策略。
 - [Windows 代码签名 (Windows Signing)](windows-signing.md) - Windows 平台 Authenticode 签名操作细节。
 - [macOS 增量更新 (macOS Delta Updates)](macos-delta-updates.md) - Sparkle 增量更新与 BinaryDelta 生成说明。
 - [macOS 打包冒烟测试 (macOS Packaged Smoke)](macos-packaged-smoke.md) - 打包后的自动化与冒烟测试检查项。
 - [发版历史与清单 (Releases Directory)](releases/) - 各版本元数据与发版记录目录。
 - [发版测试报告 (Release Reports)](release-reports/) - 各次发版的验收与冒烟测试记录。
 
 ---
 
 ## 4. OpenCodex 多模型与生态整合 (OpenCodex & Multi-Model)
 
 - [OpenCodex 多模型集成方案](OPENCODEX_MULTI_MODEL_INTEGRATION_PLAN_CN.md) - OpenCodex 多模型扩展与接入架构。
 - [OpenCodex 多模型执行与发版规划](OPENCODEX_MULTI_MODEL_EXECUTION_AND_RELEASE_PLAN_CN.md) - 落地路线图与阶段性发布计划。
 - [OpenCodex 安装闭环分析](opencodex-install-closure.md) - 安装引导、依赖检查与错误恢复闭环。
 
 ---
 
 ## 5. OSIR API 与后端边界 (OSIR API & Integration)
 
 - [OSIR API 与 Manager 职责边界](OSIR_API_MANAGER_BOUNDARY_CN.md) - 客户端与云端 API 的职责划分原则。
 - [OSIR OAuth 订阅优先接入方案](OSIRAPI_OAUTH_SUBSCRIPTION_FIRST_IMPLEMENTATION_PLAN_CN.md) - 订阅验证与 OAuth 授权首发方案。
 - [OSIR API 鉴权与多模型分析](OSIRAPI_AUTH_AND_OPENCODEX_MULTIMODEL_ANALYSIS_CN.md) - 鉴权机制与多模型代理分析。
 - [OSIR 资产全量归属评估](osir-full-ownership-assessment.md) - 资产、域名与运维权属梳理。
 - [OSIR 发版交接记录 (2026-08-17)](OSIR_RELEASE_HANDOFF_20260817_CN.md) - 早期发版与交付状态交接。
 
 ---
 
 ## 6. 管理器与统一工作台规划 (Manager Operations & SOP)
 
 - [Codex Manager SOP 规范](CODEX_MANAGER_SOP_CN.md) - 核心运维与操作标准作业程序。
 - [Codex 配置与统一工作台分析](CODEX_CONFIG_UNIFIED_WORKBENCH_ANALYSIS_CN.md) - 多平台配置统一管理工作台设计。
 - [Manager 自更新范围界定](manager-self-update-scope.md) - 客户端自更新边界与安全检查机制。
 - [Manager 更新闭环设计](manager-update-closure.md) - 自动更新状态机与错误回滚设计。
 - [会话交接记录](CODEX_MANAGER_SESSIONS_HANDOFF_CN.md) - 关键功能演进与会话交接。
 - [路线图 (Roadmap)](roadmap.md) - 历史与后续功能规划。
