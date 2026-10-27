# 开发指南

## 环境与启动

项目 CI 使用 Node.js 22、pnpm 10 和 Rust stable。还需按操作系统安装 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。macOS 与 Windows x64 为当前发布目标。

```bash
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm tauri dev` 同时启动 Vite 和桌面应用。仅开发前端时可执行 `pnpm dev`，默认地址为 `http://localhost:1030`；依赖原生 IPC 的功能需要桌面环境或专用测试夹具。

## 项目结构

| 目录 | 内容 |
|---|---|
| `src/` | React 界面、状态、国际化与 Tauri 命令封装 |
| `src/components/fileManager/` | 文件浏览、预览、操作确认与整理向导 |
| `src-tauri/src/` | Rust 扫描、SQLite 索引、搜索、文件操作、整理与存储维护 |
| `src-tauri/dmg/` | macOS 安装盘背景 |
| `scripts/` | 浏览器交互回归与 macOS 安装盘构建脚本 |
| `website/` | 官方网站静态页面 |
| `docs/` | 使用边界、设计、路线图与发布记录 |

扫描、文件读写和数据库任务在 Rust 后台线程执行。前端通过 `src/lib/tauri.ts` 调用命令，通过事件接收扫描进度；本地 SQLite 保存索引、历史摘要与操作日志。

macOS 的数据库通常位于 `~/Library/Application Support/com.rain9.luma/luma.sqlite3`。数据库属于应用数据，普通扫描不会因此变成“零磁盘写入”；存储管理与 schema 升级边界见[本地存储说明](local-storage.md)。

## 检查与回归

```bash
pnpm check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

文件管理与整理的浏览器回归方法、覆盖范围和原生操作验收要求见[文件夹整理指南](folder-organizer.md#验证)。浏览器中的模拟 IPC 检查不代替真机文件操作、权限、跨磁盘移动或安装更新验收。

人工回归应使用独立测试目录，覆盖空目录、隐藏文件、符号链接、大文件和无权限目录；核对扫描取消、目录切换、重启恢复、搜索定位、整理预览、冲突处理与重启后撤销。勿使用个人资料进行文件操作回归。

## 构建与发布

macOS 应用构建：

```bash
pnpm tauri build --bundles app
```

正式 DMG 使用独立脚本打包，保留安装背景与图标布局，具体命令见 [macOS 安装盘](macos-installer.md)。

Windows 安装包构建：

```bash
pnpm tauri build --bundles nsis,msi
```

当前配置启用了 updater 签名产物，正式构建需要提供对应签名环境变量。密钥不进入仓库、文档或日志。

发布前同步版本文件和更新说明，将验证后的提交推送至 `main`，再推送尚未发布的 `v*` 标签。Release workflow 在 macOS 和 Windows runner 完成检查与打包，再汇总发布。不要复用或覆盖已发布标签。完整流程见[发布与自动更新](updater-release.md)。

## 官网维护

官网生产地址为 <https://luma.raingpt.top/>，由 Vercel 跟随 `main` 自动部署，源码位于 `website/`。

```bash
python3 -m http.server 1031 --directory website
```

版本更新时同步官网版本、功能说明、下载链接与隐私边界。部署后核对生产域名，不能仅以本地页面或推送成功作为上线依据。纯文档与官网内容修改无需重新发布桌面安装包。
