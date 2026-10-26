# Luma 自动更新发布手册

## 当前方案

- 更新通道：GitHub Releases stable。
- 清单地址：`https://github.com/you-want/Luma/releases/latest/download/latest.json`。
- 客户端：启动时静默检查；用户可手动检查，发现新版本后点击下载、安装并重启。
- 安全：Tauri updater minisign 公钥固化在 `src-tauri/tauri.conf.json`；私钥只保存于 GitHub Actions Secret。
- 平台代码签名：Apple Developer ID、Windows Authenticode 和 macOS 公证尚未启用，与 updater 签名分开处理。

## 密钥现状（2026-10-26 更新）

- 正式密钥对已于 2026-10-26 生成并写入 `tauri.conf.json` 公钥（`C16...` 开头）。**该密钥对一经用于正式发布即永久绑定，不得重新生成或替换**——旧客户端只信任内置公钥，换钥后旧版将永远收不到更新提示。
- 私钥存放位置（两处，互为备份）：
  1. 本机：`~/.local/luma-updater/luma-updater.key`（口令在 `~/.local/luma-updater/.passphrase`，权限 600）
  2. GitHub：`TAURI_SIGNING_PRIVATE_KEY`（私钥全文，含 `-----BEGIN/END` 分隔行）+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（口令）
- 若 GitHub 侧尚未配置上述两个 Secret，在打 `v0.3.2` tag 前完成：仓库 → Settings → Secrets and variables → Actions → 新建两个 repository secret（私钥值 = `~/.local/luma-updater/luma-updater.key` 的完整内容，口令值 = `.passphrase` 文件中的字符串）。

## 发版流程

1. 确认 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 的版本策略一致；Release workflow 会在构建时以 tag 覆盖 Tauri 版本。
2. 合并发布 workflow 和 updater 代码到默认分支。
3. 推送正式 tag，例如 `v0.2.0`。
4. 等待 macOS universal 与 Windows x64 构建完成。
5. 确认 Release 同时包含：
   - macOS `.dmg`、`.app.tar.gz` 和 `.app.tar.gz.sig`
   - Windows `.msi`、`.msi.sig`、NSIS `.exe` 和 `.exe.sig`
   - `latest.json`
   - GitHub 页面可能额外显示 `Source code (zip)`/`Source code (tar.gz)`；这是 GitHub 按 tag 自动生成的链接，不属于 workflow 上传资产，无法关闭。
6. 下载并安装该版本，作为下一次真实升级测试的旧版本基线。

## 首次跨版本验收

`v0.2.0` 是第一个带 updater 的基线版本，无法独立验证“升级到自己”。完整验收需要再发布一个更高版本，例如 `v0.2.1`：

1. 在 macOS Intel、macOS Apple Silicon、Windows 11 x64 分别安装 `v0.2.0`。
2. 发布 `v0.2.1`，确认 `/releases/latest/download/latest.json` 指向新版本。
3. 启动 `v0.2.0`，确认标题栏出现 `v0.2.1` 更新提示。
4. 点击更新，确认下载进度可见、安装成功并自动重启。
5. 重启后确认应用版本为 `v0.2.1`，原有 SQLite 数据仍可读取。
6. 分别验证断网、404、下载中断、错误签名和被篡改清单；不得安装无法通过签名校验的产物。

只有三平台目标环境完成跨版本升级和失败场景验收后，才将路线图 UPDATE-002/005/007/008 标为 `DONE`。
