# Codex Manager 客户端发布操作手册

适用仓库：ousir0/osir-codex-manager
适用版本：vX.Y.Z，例如 v0.5.12

本手册固定“修改代码 → 测试 → GitHub Release → Rainyun 镜像 → 用户更新”的完整流程。发布必须基于已经提交并推送到 main 的完整 SHA，不能使用未提交工作区。

## 固定升级约定（后续默认遵循）

- Manager 的代码、配置或 OpenCodex 集成有任何修复，都必须提升 Manager 版本号；不能复用旧版本号覆盖发布。
- 用户侧升级入口是 Manager 内置更新器：发布新版本并推进 `https://app.osirclaw.com/manager/latest.json` 后，用户只需在客户端点击“立即更新”，客户端会校验版本和签名、下载安装并自动重启。
- 本机直接替换 `/Applications/Codex Manager.app` 只用于开发验收，不作为用户发布方式；正式用户更新必须走版本化 Release + `latest.json`。
- 发布完成后必须用旧版本客户端验证一次“发现更新 → 点击更新 → 自动重启 → 关于页版本变为目标版本”。

## 1. 发布前检查

    cd /Users/ouwei/codex-app-manager
    git status --short --branch
    git diff --check
    git log -1 --format='%H %s'

工作区必须干净。版本声明检查：

    node scripts/check-release-version.mjs source vX.Y.Z .

检查范围包括 package.json、package-lock.json、src-tauri/Cargo.toml、src-tauri/Cargo.lock 和 src-tauri/tauri.conf.json。

## 2. 本地质量门

    npm run check
    npm test -- --run
    npm run test:release
    cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked
    npm run build
    git diff --check

Windows x64/ARM64 和 macOS 双架构必须以 GitHub Actions 结果为准。本地 macOS 不能替代 Windows 干净环境验证。

仓库中的压缩课件或第三方打包资源可能触发既有 lint 报错；遇到这类文件时要记录原因，不要为了发布临时修改压缩资源。

## 3. 升级版本并创建标签

以目标版本替换 0.5.12：

    npm version 0.5.12 --no-git-tag-version
    cargo update -p osir-codex-manager --offline
    node scripts/check-release-version.mjs source v0.5.12 .
    git diff --check
    git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
    git commit -m "release: v0.5.12"
    git push origin main
    git tag -a v0.5.12 -m "Codex Manager v0.5.12"
    git push origin v0.5.12

只给已经推送到 main 的提交创建标签。Release 失败时优先重新运行对应的 GitHub Actions，不要重复创建同名标签。

## 4. 监控 GitHub Release

    gh run list --repo ousir0/osir-codex-manager --limit 10
    gh run watch RELEASE_RUN_ID --repo ousir0/osir-codex-manager --exit-status
    gh release view v0.5.12 --repo ousir0/osir-codex-manager --json tagName,isDraft,isImmutable,assets

正式 Release 必须满足：isDraft=false、isImmutable=true，并包含 Windows 两套安装包及签名、macOS 两套 DMG/tar.gz 及签名、latest.json、SHA256SUMS 和 release-binding.json。

## 5. 推进 Rainyun Manager 镜像

每次远端操作前执行：

    cd /Users/ouwei/sub2api-new
    tools/audit_rainyun_connection_targets.sh

然后执行统一入口：

    cd /Users/ouwei/codex-app-manager
    npm run publish:manager -- v0.5.12

该入口会下载不可变 GitHub Release、校验 SHA-256、复制当前线上 site、上传完整 Manager 资产、校验签名文件并原子切换 current。同版本重复执行会直接幂等退出。

Rainyun 固定目标是 root@100.82.197.6，禁止使用公网 IP、临时 SSH 别名或其他服务器。

## 6. 线上回读验收

    curl -fsSL https://app.osirclaw.com/manager/latest.json | jq -r '.version, (.platforms | keys[])'

版本必须等于目标版本，并包含 darwin-aarch64、darwin-x86_64、windows-x86_64、windows-aarch64。

    for name in CodexManager_0.5.12_x64-setup.exe CodexManager_0.5.12_arm64-setup.exe CodexManager_aarch64.dmg CodexManager_x86_64.dmg; do
      curl -L -sS -o /dev/null -w "$name %{http_code} %{size_download}\n" "https://app.osirclaw.com/manager/latest/$name"
    done

所有下载入口必须返回 200。还要确认 GitHub Release 与 Rainyun 资产 SHA-256 一致，并在客户端点击“立即更新”验证发现目标版本。

## 7. 失败处理与回滚

GitHub Release 构建失败时，不推进线上 latest，不删除旧版本；修复后提交新版本再发布。

Rainyun 镜像失败时，脚本不得切换 current。旧版本目录必须保留，残留 uploading 目录需要先只读确认再清理。

回滚前先读取当前目录：

    source /Users/ouwei/sub2api-new/tools/rainyun_ssh_target.sh
    ssh "$(rainyun_resolve_ssh_host)" 'readlink -f /var/www/osir-codex-manager/current; find /var/www/osir-codex-manager/releases -maxdepth 1 -mindepth 1 -type d | sort'

确认上一版本后执行原子切换：

    source /Users/ouwei/sub2api-new/tools/rainyun_ssh_target.sh
    ssh "$(rainyun_resolve_ssh_host)" 'ln -s /var/www/osir-codex-manager/releases/PREVIOUS_VERSION /var/www/osir-codex-manager/.current-rollback && mv -Tf /var/www/osir-codex-manager/.current-rollback /var/www/osir-codex-manager/current'

回滚后重新检查 latest.json、四个平台 URL 和 current 指向。不要删除旧版本目录。

## 8. 发布记录

每次发布至少记录：版本、完整源码 SHA、GitHub Run ID、四个平台构建结果、GitHub Release 状态、Rainyun current 真实目录、latest.json 版本、回滚目录和异常原因。

## 最终清单

- [ ] 工作区干净，版本声明一致。
- [ ] 本地检查、单测、发布测试、Rust 测试和生产构建通过。
- [ ] main 已推送，标签指向正确完整 SHA。
- [ ] GitHub Release 正式发布且不可变。
- [ ] 四个平台资产、签名、latest.json、SHA256SUMS 齐全。
- [ ] Rainyun Tailscale 审计通过。
- [ ] Rainyun 镜像完成且 current 原子切换成功。
- [ ] latest.json 返回目标版本。
- [ ] 四个平台下载 URL 返回 200。
- [ ] 客户端“立即更新”发现并安装目标版本。
- [ ] 发布 SHA、Run ID、current 和回滚目录已记录。
