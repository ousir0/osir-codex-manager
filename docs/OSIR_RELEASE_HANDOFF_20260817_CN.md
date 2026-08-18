# Codex Manager 独立化发布交接（OSIR-owned）

## 当前结论

客户端源码、产品名、二进制名、Bundle ID、Logo、安装器图片、API、下载、更新、皮肤、历史镜像、国际化 relay、GitHub 工作流和 updater 公钥已经迁移到 OSIR 体系。旧 AWAI 标识只保留在带审计豁免的自动迁移兼容代码中。

## 已完成

- 客户端显示名：`Codex Manager`。
- 二进制：`osir-codex-manager`。
- Bundle ID：`com.osir.codexmanager`。
- GitHub：`ousir0/osir-codex-manager`，私有仓库。
- API：`https://api.osirclaw.com/v1`。
- 应用、下载、更新、皮肤和 relay 主域：`https://app.osirclaw.com`。
- 新 updater 密钥对已生成；私钥不在仓库，公钥已写入客户端。
- GitHub `release` Environment 已配置 updater Key、密码和发布校验 Secret。
- 旧 Manager 数据目录可自动迁移到 OSIR 目录。
- 旧 `awai` Provider 可迁移为 `osir`，保留用户 API Key。
- 全套原始 Codex 演化图标已复用并生成，包括 macOS、Windows、移动端、README 和 NSIS 图片；应用界面不展示独立 OSIRAPI Logo。
- OSIR 网站和 macOS 候选包已上传至服务器独立目录：
  `/var/www/osir-codex-manager/releases/20260817-osir-codex-manager-0.5.3`。
- Nginx HTTP 候选入口已启用，不影响现有主站和 API。
- 阿里云 DNS 已创建 `app.osirclaw.com -> 154.40.47.227`。
- Let's Encrypt HTTPS 证书已签发，当前有效期至 2026-11-16，并启用自动续期。
- `app.osirclaw.com` 的官网、健康检查、更新清单和 Range 下载已通过公网验证。
- 独立 i18n relay 已运行在 `127.0.0.1:3130`，公网 WebSocket 验证返回 `ready`。

## macOS 候选工件

| 文件 | SHA-256 |
|---|---|
| `CodexManager_aarch64.dmg` | `6ebdcdee45a8d47ee9bd9e7635d0bd160b6e9a67ba0c773858ada0f8646e4bb9` |
| `CodexManager_aarch64.app.tar.gz` | `ef7741557fe3c2222bba6849e7cd3b78f0c1b99574c2162f4f9a7fd087f5784d` |
| `CodexManager_x86_64.dmg` | `5da217f3173a68e52acf6e3c32290ed5826aa8477a0929ab6bc9fa5b561e5de6` |
| `CodexManager_x86_64.app.tar.gz` | `7e64e49a7e11ddc4896a6b7e9aa2e213720e1cbe41bf5289be27d2488596427c` |

包内已验证：

- `CFBundleName=Codex Manager`。
- `CFBundleIdentifier=com.osir.codexmanager`。
- `CFBundleExecutable=osir-codex-manager`。
- 主二进制为 Apple Silicon arm64。
- updater tarball 使用 OSIR updater 私钥签名。
- 包内没有旧品牌、旧域名或旧 Bundle ID。

## 服务器验证

在 DNS 生效前，通过服务器 IP + `Host: app.osirclaw.com` 已验证：

- `/health` 返回 `200`。
- 官网首页返回 OSIR 标题。
- `/manager/latest.json` 返回 OSIR `0.5.3` 部分发布清单。
- DMG Range 请求返回 `206`。
- 远端工件 SHA-256 与本地一致。

## 尚未完成的发布级门槛

1. 本机没有 Apple Developer ID，当前 macOS 包是可验证的 ad-hoc 测试签名，尚未公证。
2. Windows Authenticode 证书尚未配置；Windows x64 需要 GitHub Actions 构建并下载验收。
3. 当前服务器候选 `latest.json` 已包含 macOS arm64 与 Intel，Windows 两个架构工件补齐后才能取消 `partial`。
4. 腾讯云 COS 已在 OSIRAPI 生产环境配置，但 Manager 工件的独立前缀、生命周期和 GitHub Secret 尚未启用；当前由服务器 + GitHub Releases 承载。
5. 正确图标版本的 Windows Runs `32064417228`、`32065194564`、`32098144600` 均在 Runner 分配阶段失败，未执行构建步骤、未产生可用工件。

## 验证命令

```bash
npm run audit:ownership:strict
npm run check
npm run lint
npm test
npm run test:release
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
git diff --check
```
