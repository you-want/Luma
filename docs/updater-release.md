# Luma 自动更新发布手册

## 当前发布与验收（2026-10-27）

- 正式版本：[v0.4.0](https://github.com/you-want/Luma/releases/tag/v0.4.0)，发布提交 `e13e37f`。
- [发布流水线](https://github.com/you-want/Luma/actions/runs/40431077912)的 macOS universal、Windows x64 构建与发布任务均成功，Release 包含 8 个资产。
- 稳定版清单：`https://github.com/you-want/Luma/releases/latest/download/latest.json`，发布核验时版本为 `0.4.0`，覆盖 macOS Intel / Apple Silicon 与 Windows x64 的安装目标。
- 用户于 2026-10-27 确认应用内 `v0.3.2 → v0.4.0` 升级成功。该反馈证明一台现用设备的正常升级链路可用；平台、架构及升级后数据逐项核验未单独记录。
- 尚未完成：全部目标设备的升级矩阵，Windows 真机安装/卸载/权限回归，以及坏签名、清单篡改、断网、下载中断、空间不足和安装失败场景。不能据一次成功升级将这些验收标记完成。

## v0.4.1 发布准备

包含本地索引空间管理、普通移动确认和新版 macOS 安装盘。前端 6 项、Rust 63 项测试通过，rustfmt、Clippy 与生产前端构建通过；安装界面经用户确认。等待双平台 CI、Release 资产和稳定版更新入口核验。

## 更新机制与签名

应用启动时静默检查 GitHub Releases stable 通道；用户也可手动检查，点击更新后下载、安装并重启。更新检查与安装包下载需要联网，不上传用户文件、索引或操作日志。

`latest.json` 包含版本、发布说明、各目标 URL 和安装包的 minisign 签名。客户端使用 `src-tauri/tauri.conf.json` 固定的公钥校验下载产物；清单自身不另行签名。发布说明来自 `docs/releases/<tag>.md`，当前客户端通过更新按钮的悬浮提示展示说明，独立的更新说明面板仍待完善。

正式签名密钥于 2026-10-26 启用，自 v0.3.2 起用于当前发布链路。已发布客户端信任内置公钥，不能直接替换密钥；轮换必须单独设计兼容迁移。

- GitHub Actions 已配置 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；v0.3.2 / v0.4.0 成功发布是现有签名链路的运行证据。
- 本机备份位于 `~/.local/luma-updater/`，私钥与口令不进入仓库、文档或日志。
- Apple Developer ID、Windows Authenticode 和 macOS 公证尚未启用，与 updater 产物签名分开处理。首次安装仍可能提示来源未验证。

## 发版流程

1. 同步 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 与 `src-tauri/tauri.conf.json` 的版本，新增 `docs/releases/<tag>.md`。workflow 仍会用 tag 覆盖 Tauri 构建版本。
2. 完成 README、官网与路线图同步，运行项目既有前端/Rust 检查及受影响的交互回归。
3. 将已验证的发布提交推送到 `main`，再推送尚不存在的正式 tag，例如后续补丁版本 `v0.4.1`。不重写已经公开发布的 tag。
4. 等待 macOS universal 与 Windows x64 构建成功，随后 `publish` job 汇总两平台产物并发布 GitHub Release。
5. 核对 8 个资产：macOS `.dmg`、`.app.tar.gz`、`.app.tar.gz.sig`；Windows `.msi`、`.msi.sig`、NSIS `.exe`、`.exe.sig`；以及 `latest.json`。GitHub 自动显示的源码压缩包不计入上传资产。
6. 核对正式 Release、稳定版更新入口与各平台下载 URL；区分 CI 构建通过、已发布、下载核验和真机安装/升级验收。
7. 在目标设备从已安装旧版本升级，记录设备系统与架构、版本、数据保留情况和结果，并同步下表。

## 升级验收矩阵

| 验收项 | 当前证据 | 状态 |
|---|---|---|
| 一台现用设备正常升级 | 用户于 2026-10-27 确认 v0.3.2 → v0.4.0 成功；未单独记录平台/架构 | 已通过 |
| macOS Intel / Apple Silicon 分别升级 | universal 构建成功；缺少按架构记录的真机结果 | 待补 |
| Windows x64 正常升级 | `.msi` / NSIS 与签名产物已发布；缺少真机结果 | 待补 |
| 升级后扫描索引与操作日志保留 | 用户尚未逐项确认数据保留；数据库迁移自动测试通过 | 待补真机验收 |
| 错误签名、被篡改清单与下载产物 | 使用 updater 公钥校验机制；尚未记录故障注入结果 | 待补 |
| 断网、404、下载中断、磁盘不足、安装失败 | 尚未记录逐场景结果 | 待补 |

UPDATE-002 / UPDATE-005 的完成依据是签名配置及生产发布；UPDATE-007 保持部分验证，UPDATE-008 保持待验收。后两项只有在对应目标设备和失败场景完成后才能标记完成。
