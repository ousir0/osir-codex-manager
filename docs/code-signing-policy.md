# 代码签名政策 · Code Signing Policy

生效日期 / Effective date: 2026-08-15

## 当前状态

- macOS 正式构建使用 Developer ID、hardened runtime 和 Apple notarization（发布流水线
  配置完成后才会对外宣称已公证）。
- Windows 安装器当前可能没有 Authenticode。首次运行出现 SmartScreen 提示是预期情况，
  不能用 Tauri updater 签名替代 Windows 发行者签名。
- Manager 自更新使用 Tauri updater 签名；该签名只认证更新文件字节，不认证 Windows
  发布者身份。
- SignPath Foundation 申请仍以公开审核结果为准。在真正接入、签名并验证发布工件之前，
  项目页面必须明确写“未完成 Authenticode 签名”，不得暗示已经获批。

## 我们签什么

本政策只覆盖从本公开 MIT 仓库构建的 AWAI Codex App Manager 可执行文件和安装器。它不
覆盖 OpenAI Codex 本体、用户文件、皮肤素材或其他第三方二进制。Manager 不重新打包或
重新签署官方 Codex；官方包仍由其原始签名负责身份验证。

启用 Windows 生产签名后，签名必须覆盖最终发布的主程序、卸载器和安装器，并使用可信
时间戳。Tauri `.sig` 必须在 Authenticode 签名完成后重新生成，GitHub Release、镜像和
校验清单必须引用同一批最终字节。

## 发布闸门

1. 构建来自审查过的默认分支提交和受保护的 release tag。
2. CI 记录版本、目标平台、源码提交、工件哈希和构建日志。
3. 发布前验证 PE/Mach-O 架构、原生签名、Tauri updater 签名、SHA-256 和安装冒烟。
4. 任意签名、来源、时间戳、恶意软件扫描或签后校验失败都会阻止发布。
5. 只有上传后的工件完成回读并与本地哈希一致，镜像 `latest` 指针才可更新。

## 账户与审批

源码仓库、GitHub Actions、镜像凭据和未来的签名服务都使用最小权限，并启用 MFA。发布
审批至少由维护者检查变更、CI 结果、版本说明和工件摘要后完成。私钥、PFX、API token
和镜像写入凭据只能放在受保护的 secrets/environment 中，不得提交到仓库或日志。

如果 SignPath Foundation 最终批准并启用生产签名，发布页面会按其要求展示以下归属（仅
对实际签名版本适用）：

> Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/)

## 用户核验

从 [AWAI 镜像](https://codexapp.awai.cc) 或 [GitHub Releases](https://github.com/qq501987847/codex-app-manager/releases)
下载后，请用同一版本的 `SHA256SUMS` 检查文件，再按平台验证 Authenticode、Developer
ID 或 Sparkle 签名。不要仅凭文件名、下载页面或 SmartScreen 文案判断来源。

相关实施说明见 [`windows-signing.md`](./windows-signing.md) 和 [`release.md`](./release.md)。
许可证文本见仓库根目录的 [`LICENSE`](../LICENSE)；MIT 允许修改、再发布和商用，但必须
保留原版权声明与许可文本。
