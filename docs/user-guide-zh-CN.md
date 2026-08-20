# Codex Manager 用户使用手册

本手册只说明用户如何完成操作，不要求用户了解 Tauri、Rust、OpenCodex 或发布系统。

## 1. 你需要先知道什么

Codex Manager 用来管理官方 Codex 桌面应用，包含：

- 安装和更新 Codex；
- 管理 Codex 配置和 API Key；
- 安装和连接 OpenCodex 多模型；
- 切换 Codex 主题；
- 更新 Manager 自身；
- 失败时恢复配置或保留旧版本。

Manager 不会上传你的对话、工作区、API Key 或本地配置。

## 2. 下载正确安装包

打开官网的“下载”页面，根据电脑系统选择：

| 设备 | 安装包 |
| --- | --- |
| Apple Silicon Mac（M1/M2/M3/M4） | macOS Apple Silicon |
| Intel Mac | macOS Intel |
| 普通 Windows 电脑 | Windows x64 |
| Windows ARM 电脑 | Windows ARM64 |

不确定 Windows 架构时，可在“设置 → 系统 → 系统类型”查看；显示“基于 x64 的处理器”时选择 Windows x64，显示 ARM64 时选择 Windows ARM64。

## 3. 安装 Manager

### macOS

1. 打开 DMG 文件；
2. 将 Codex Manager 拖到 Applications；
3. 从 Applications 启动；
4. 如果系统第一次提示来源，确认你下载的是官网或对应 GitHub Release 的安装包，再按系统提示打开。

### Windows

1. 打开 EXE 安装包；
2. 按安装器提示继续；
3. 安装完成后启动 Codex Manager；
4. 如果 SmartScreen 出现提示，先核对下载来源和 SHA-256，再决定是否继续。

## 4. 第一次启动

启动后，Manager 会检查系统架构、Codex 安装状态、版本、来源和可恢复配置。

页面显示“已是最新”时，可以直接启动 Codex。页面显示“需要更新”时，先查看目标版本、下载大小和更新说明，再点击“立即更新”。

## 5. 安装 Codex

如果页面显示“未检测到 Codex”：

1. 点击“安装 Codex”；
2. 确认目标版本和安装方式；
3. 等待下载和校验完成；
4. 等待安装结束；
5. 看到“安装完成”后启动 Codex。

如果安装失败，Manager 会保留当前可运行版本，不会用半成品覆盖旧安装。先查看错误详情，再点击“重新检查”或“重试”。

## 6. 配置 Codex API

进入“设置 → Codex 配置管理”。

### 普通聊天 API

1. 打开“连接与模型”；
2. 选择供应商；
3. 填写 Base URL；
4. 填写模型名称；
5. 输入 API Key；
6. 点击“保存并启用”。

保存后，Manager 会验证配置格式并保留上一版备份。API Key 只显示“已配置”，不会在页面回显。

### OSIR API

如果使用 OSIR API：

- API 地址：api.osirclaw.com/v1；
- 在 OSIR API 控制台创建或管理 API Key；
- 在 Manager 中把 API Key 保存到本机；
- 使用“获取模型”读取当前可用模型。

OSIR API 是独立的网站和服务入口，不需要把 API Key 粘贴到公开网页或聊天窗口中。

## 7. 安装并连接 OpenCodex 多模型

进入“设置 → Codex 配置管理 → OpenCodex 多模型”。

### 完全没有 OpenCodex

点击“安装多模型组件”。Manager 会自动检查系统和 CPU，优先使用对应平台的自带组件；如果缺少 Node/npm，会准备 Manager 私有运行时，然后安装 OpenCodex、启动本机服务并等待健康状态变为“已就绪”。

你不需要自己安装 Node、npm 或执行命令。

### 已经有部分环境

如果本机已经有 Node、npm 或 OpenCodex，Manager 会优先识别并复用可用环境。已有环境不可用时，Manager 会转为私有运行时，不修改全局环境。

### 连接 OSIR API

1. 点击“连接 OSIR API”；
2. 点击“浏览器登录并连接”；
3. 在浏览器完成登录授权；
4. 回到 Manager，等待账户、订阅、路由和模型同步；
5. 看到“已连接，可使用订阅模型”后完成。

Manager 只有在本机服务、账户、模型目录和默认路由都验证成功后，才会显示连接成功。

## 8. 主题

进入“设置 → 主题”。浏览主题，点击“试穿”预览，确认后点击“应用”。如果页面提示需要重启 Codex，请完全退出并重新打开 Codex。

主题不会修改 Codex 应用文件。应用失败时，可以点击“恢复原生外观”。

## 9. Manager 自动更新

Manager 会在启动或定时检查时查询新版本。发现新版本后，查看当前版本、目标版本和更新说明，点击“更新”，等待下载、签名校验和自动重启，最后在“关于”页确认目标版本。

你也可以进入“设置 → 关于 → 检查管理器更新”手动检查。

“跳过当前版本”只会跳过这个具体版本；以后发布更高版本时仍会提醒。

## 10. 更新失败怎么办

### 无法连接更新服务器

检查网络、检查“设置 → 网络 → 代理”、稍后再次检查。不要下载来源不明的替代包。

### 签名校验失败

不要继续安装。关闭提示，保留当前版本，等待官网或 GitHub Release 发布修复版本。

### 更新后仍显示旧版本

完全退出 Manager 后重新打开，再进入“关于”查看版本。如果仍未变化，使用对应 GitHub Release 的安装包手动覆盖安装。

### OpenCodex 连接失败

进入 OpenCodex 页面查看环境状态、服务状态和错误说明。可以依次点击重新连接、重启 OpenCodex、同步模型或恢复备份。

## 11. 卸载

进入“设置 → 卸载 Codex”。卸载前确认是否保留 Codex 数据。保留数据后，重新安装可以继续使用登录状态和配置；清除数据会删除本地登录、会话和配置，不能恢复。

## 12. 语言切换

### Manager

进入“设置 → 外观 → 语言”，选择中文或 English。切换后界面、菜单和提示会同步更新。

### 官网

点击导航栏右侧的语言按钮，在中文和 English 之间切换。选择会保存在当前浏览器中。

### GitHub 文档

完整手册分别位于：docs/user-guide-zh-CN.md 和 docs/user-guide-en.md。

## 13. 去哪里找帮助

- 下载与版本：Codex Manager 官网；
- API Key、模型和额度：OSIR API 控制台；
- API 请求格式：OSIR API 文档；
- 软件问题和源码：GitHub 仓库；
- 主题：Codex Manager skins 仓库。

反馈时请提供版本、系统、CPU 架构和错误摘要，不要上传 API Key、完整配置或对话内容。
