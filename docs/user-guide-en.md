# Codex Manager User Guide

This guide explains what users need to do. You do not need to know Tauri, Rust, OpenCodex, or the release system.

## 1. What Codex Manager does

Codex Manager manages the official Codex desktop app: install and update Codex, manage configuration and API keys, install OpenCodex multi-model routing, preview themes, update Manager itself, and recover from failed operations.

Manager does not upload your conversations, workspace files, API keys, or local configuration.

## 2. Choose the right installer

On the website Download page, choose the package for your system:

| Device | Package |
| --- | --- |
| Apple Silicon Mac (M1/M2/M3/M4) | macOS Apple Silicon |
| Intel Mac | macOS Intel |
| Regular Windows PC | Windows x64 |
| Windows ARM PC | Windows ARM64 |

If you are unsure about Windows, open Windows Settings → System → About and check the system type.

## 3. Install Manager

### macOS

1. Open the DMG file;
2. Drag Codex Manager to Applications;
3. Launch it from Applications;
4. If macOS shows a first-launch warning, verify that the package came from the official website or the matching GitHub Release before opening it.

### Windows

1. Open the EXE installer;
2. Follow the installer steps;
3. Launch Codex Manager after installation;
4. If SmartScreen appears, verify the source and SHA-256 before deciding whether to continue.

## 4. First launch

Manager checks the operating system, CPU architecture, Codex installation, version, source, and recoverable configuration. If the page says Up to date, you can launch Codex. If an update is available, review the target version, download size, and release notes before updating.

## 5. Install Codex

If Codex was not detected:

1. Click Install Codex;
2. Confirm the target version and installation method;
3. Wait for download and verification;
4. Wait for installation to finish;
5. Launch Codex after the success state appears.

If installation fails, Manager keeps the current runnable version instead of replacing it with a partial package. Read the error details, then retry or run the check again.

## 6. Configure a Codex API

Open Settings → Codex configuration. Choose a provider, enter the Base URL, model, and API key, then click Save and enable. Manager validates the configuration and keeps a previous-version backup. Saved API keys are shown only as configured or missing.

For OSIR API:

- API endpoint: api.osirclaw.com/v1;
- create or manage API keys in the OSIR API console;
- save the key locally in Manager;
- use Fetch models to read the available models.

OSIR API is a separate website and service. Never paste an API key into a public webpage or chat window.

## 7. Install and connect OpenCodex multi-model

Open Settings → Codex configuration → OpenCodex multi-model.

When OpenCodex is not installed, click Install multi-model component. Manager detects the operating system and CPU, uses the matching managed component when available, prepares a private runtime if Node/npm is missing, installs OpenCodex, starts the local service, and waits until the health state is ready. You do not need to install Node, npm, or run commands yourself.

If part of the environment already exists, Manager detects and reuses an available Node/npm or OpenCodex installation. If it is incomplete or unhealthy, Manager falls back to a private runtime without changing the global environment.

To connect OSIR API:

1. Click Connect OSIR API;
2. Click Sign in and connect in browser;
3. Complete authorization in the browser;
4. Return to Manager and wait for account, subscription, route, and model sync;
5. Finish only after the connected state appears.

Manager reports success only after the local service, account, model catalog, and default route pass verification.

## 8. Themes

Open Settings → Themes. Browse themes, click Preview, and click Apply when ready. Restart Codex completely if the page asks you to. Themes do not modify the Codex application bundle. Use Restore native appearance if an application fails.

## 9. Manager self-update

Manager checks for updates at startup and on the configured schedule. When an update is found, review the current and target versions, read the release notes, click Update, wait for download and updater-signature verification, let Manager restart, and confirm the target version in About.

You can also open Settings → About → Check for manager updates. Skip version applies only to that exact version. A later version will still be offered.

## 10. Troubleshooting

### Update server is unreachable

Check the network, check Settings → Network → Proxy, and try again later. Do not use an unknown replacement package.

### Signature verification failed

Do not continue the installation. Keep the current version and wait for a corrected release from the official website or GitHub Release.

### The old version is still shown after updating

Quit Manager completely, reopen it, and check About. If it is still old, install the matching GitHub Release package manually.

### OpenCodex connection failed

Open the OpenCodex workspace and inspect the environment, service, and route states. Try reconnecting, restarting OpenCodex, syncing models, or restoring the previous backup.

## 11. Uninstall

Open Settings → Uninstall Codex. Choose whether to keep Codex data. Keeping data preserves login, sessions, and configuration for a later reinstall. Purging data removes local login, session, and configuration data permanently.

## 12. Language switching

### Manager

Open Settings → Appearance → Language and choose 中文 or English. The interface, menus, and status messages update together.

### Website

Use the language button on the right side of the navigation bar. The choice is saved in the current browser.

### GitHub documentation

The complete guides are docs/user-guide-zh-CN.md and docs/user-guide-en.md.

## 13. Where to get help

- downloads and releases: the Codex Manager website;
- API keys, models, and quotas: the OSIR API console;
- request format: OSIR API documentation;
- source and software issues: the GitHub repository;
- themes: the Codex Manager skins repository.

When reporting a problem, include the version, operating system, CPU architecture, and a short error summary. Do not upload API keys, full configuration files, or conversation data.
