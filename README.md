# Luma

Luma 是一个本地优先的文件空间观察与整理工具，当前已验证 macOS 平台，Windows 11 x64 代码适配已完成。它扫描用户明确选择的目录，在本机建立 SQLite 索引，并展示空间分类、最大文件、重复文件候选、开发项目与可解释的复查线索。普通扫描只读取路径和元数据；只有用户主动查找重复、预览内容或提交文件操作时，Luma 才会读取内容或修改文件。文件内容、索引和操作日志均不上传。

当前正式版本：[`v0.4.0`](https://github.com/you-want/Luma/releases/tag/v0.4.0)（2026-10-27 已发布）

官网：[luma.raingpt.top](https://luma.raingpt.top/)。Release 已提供 macOS universal `.dmg` 和 Windows x64 `.msi` / NSIS `.exe`，附带签名 updater 产物。用户已于 2026-10-27 确认应用内 `v0.3.2 → v0.4.0` 升级成功；该设备的平台与架构未单独记录，其他设备及 Windows 真机验收仍需补齐。

本版更新见 [`docs/releases/v0.4.0.md`](docs/releases/v0.4.0.md)。v0.4.1 发布准备包含本地存储管理、移动确认与新版 macOS 安装界面，见 [`docs/local-storage.md`](docs/local-storage.md)。

后续功能与逐项进度见 [`docs/product-roadmap.md`](docs/product-roadmap.md)；Search v2 的拆解、验收和持续执行记录见 [`docs/search-v2-plan.md`](docs/search-v2-plan.md)。

## 能做什么

- **只读目录扫描** — 选择单个目录递归扫描，默认忽略隐藏项且不跟随符号链接；可选是否包含隐藏文件、是否跨文件系统。
- **实时进度与取消** — 实时展示文件数、目录数、已读取大小、当前路径与错误数（约每 100 毫秒节流一次）。可随时取消；单个文件无权限、消失或元数据损坏只计入错误数，不中断整个任务。
- **空间概览** — 按分类展示占用的分段条（图片、视频、音频、文档、压缩包、代码、应用、其它），风格接近系统设置里的存储视图。
- **搜索与过滤** — 使用本地 SQLite FTS5 搜索当前扫描快照，支持相关度排序、分类/扩展名/大小/时间过滤和分页；可识别“最近一年 PDF”“大于 1GB”“下载目录”“代码项目”等确定性条件，搜索结果可在 Finder 中定位。
- **批量选择与清单导出** — 在搜索结果中按项或跨分页选择文件，支持全选当前页与「全部筛选结果」两种语义，可复制路径或导出 CSV；搜索页本身不执行文件写入。
- **统一目录工作区** — 顶部共用目录选择、桌面/下载快捷入口与扫描操作，下方切换空间分析和文件管理；切换保留子目录与滚动位置，更换目录后需重新扫描。
- **文件管理** — 在扫描索引中浏览目录、预览图片/文本/PDF，并在主动提交后重命名或复制，移动和移至废纸篓需确认；同名目标不会被覆盖。
- **文件整理** — 提供桌面、下载目录快捷选择，以及截图、表格、演示文稿、安装包等细分类；也支持按类型、年份、年月、扩展名和文件名关键词生成计划。默认仅整理当前层散文件，保留已有文件夹与隐藏文件。分组预览中可取消勾选、修改分类名称或单个文件去向，同名冲突可跳过或自动编号；修改方案后必须重新预览，确认后才执行。整理与普通移动均记录本地日志并支持跨重启撤销。使用方式与边界见 [`docs/folder-organizer.md`](docs/folder-organizer.md)。
- **最大文件** — 列出前 20 个最大文件，可在 Finder 中定位。
- **”值得留意”发现与明细下钻** — 按可调阈值发现五类项目：超大文件、长期未修改、开发构建内容、压缩包、应用与安装包。每一类都能点开查看其中最大的前 10 个文件并在 Finder 中定位。明细列表与汇总数字复用同一套判定规则，两者始终一致。
- **重复文件候选** — 先按文件大小筛选，再对候选文件计算 BLAKE3 内容哈希，展示重复组、可节省空间和 Finder 定位入口。只有用户主动执行查找时才读取文件内容。
- **开发项目识别** — 识别 Node.js、Rust、Python、Git、Xcode、Maven 和 Gradle 项目，展示项目类型、文件数、占用空间与路径。
- **扫描历史对比** — 按目录保留最近 3 次终态扫描，可比较总容量、文件数量和分类变化。
- **本地持久化** — 结果保留在本机 SQLite，启动后自动恢复最近一次成功结果。v0.4.1 仅保留最近一次成功扫描的完整索引，历史转为统计摘要，每目录最多 3 次、全局最多 30 次；单次扫描新增索引预算 256 MiB。顶部「本地存储」可查看占用、回收空闲空间或确认清理索引，撤销记录独立保留。完整索引不进入前端状态。
- **中英文界面** — 内置简体中文与英文，首次启动按系统语言自动选择，可在标题栏手动切换并持久化保存；数字、容量与日期随语言本地化。

## 安全与隐私

Luma 的普通扫描只读取文件路径和元数据（名称、扩展名、大小、修改时间），不会修改、移动或删除被扫描的文件。重复检测和内容预览是显式操作，读取仅发生在本机，文件内容和哈希不会上传。

对用户原文件的写入能力只出现在文件管理视图，并遵守以下边界：

- 移至废纸篓、重命名、复制、移动和规则整理都必须由用户主动触发；规则整理在执行前展示完整计划。
- 系统目录、应用目录和操作系统关键路径禁止修改；整理源目录必须位于当前扫描范围内。
- 执行前会重新校验文件大小和修改时间；扫描后发生变化的文件会被拒绝，要求重新扫描。
- 不覆盖同名目标。跨磁盘移动采用复制后删除源文件，并在数据库更新失败时尽力回滚文件系统变化。
- 重命名、移动和规则整理的成功项逐条写入 SQLite 操作日志；应用意外退出后，已完成部分仍可在重启后撤销。复制和移至废纸篓当前不提供应用内撤销。

索引与操作日志写入 Tauri 应用数据目录；用户主动提交的文件操作会修改对应文件。macOS 上数据库通常位于：

```text
~/Library/Application Support/com.rain9.luma/luma.sqlite3
```

“大文件”“长期未修改”等发现只表示符合透明规则，不能据此判断文件可以安全删除。修改时间也不等于最近使用时间。

## 技术栈

- **前端** — React 19 + TypeScript + Vite，`lucide-react` 图标；轻量的 `useSyncExternalStore` store，无第三方状态库；`i18next` + `react-i18next` 负责中英文资源与类型化文案键。界面支持简体中文与英文（`i18next` + `react-i18next`），默认跟随系统语言，可在标题栏手动切换并持久化。
- **后端** — Rust + Tauri 2；`walkdir` 遍历、`rusqlite`（bundled SQLite）持久化、`uuid` 生成扫描 ID。
- **进程模型** — 扫描在 Rust 阻塞任务中执行，文件分批（每批 500 条）写入 SQLite，进度通过事件回传前端。前端经 `src/lib/tauri.ts` 统一调用后端命令。

## 项目结构

```text
src/                    前端
  app/store.ts          全局状态（useSyncExternalStore）
  components/           UI 组件（扫描控制、进度、概览、分类、最大文件、搜索、发现、重复、项目、历史）
  hooks/useScanEvents   订阅后端扫描进度/完成事件
  lib/tauri.ts          后端命令封装
  lib/format.ts         字节/数字/日期格式化（随语言切换的 Intl 本地化）
  lib/errors.ts         按错误 code 翻译后端 AppError
  i18n/                 i18next 初始化、语言检测/持久化
  i18n/locales/         zh-CN、en-US 资源（类型化键，含完整性测试）
  contexts/             SelectionContext（跨分页批量选择状态）
  types/scan.ts         前后端共享类型
src-tauri/src/          后端
  scanner.rs            目录遍历与文件分类
  database.rs           SQLite schema、写入与查询（含搜索）
  commands.rs           Tauri 命令入口
  models.rs             序列化数据模型
  i18n.rs               托盘/系统字符串本地化（按启动时系统语言）
```

## 开发

需要 Node.js、pnpm、Rust 和 Tauri 2 的 macOS 系统依赖。

```bash
pnpm install
pnpm tauri dev
```

发布前的检查：

```bash
pnpm check                                                              # 前端测试 + 构建
cargo fmt   --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml
```

## 构建与分发

```bash
pnpm tauri build
```

产物位于 `src-tauri/target/release/bundle/`：macOS 为 `.app`/`.dmg`，Windows 为 `.msi`/NSIS `.exe`。

macOS 正式安装盘使用独立脚本生成品牌背景、128 点图标和拖拽引导，避免 Finder 自动化未保存外观设置的问题。CI 已接入；本地构建方法见 [macOS 安装盘](docs/macos-installer.md)。

### 打 tag 自动发版

`.github/workflows/release.yml` 会在推送 `v*` 形式的 tag 时，在 macOS 与 Windows runner 分别打包并发布到同一个 GitHub Release：

```bash
git tag v0.4.1
git push origin v0.4.1
```

流程说明：

- 在 macOS runner 上构建 `universal-apple-darwin` 通用二进制，一份产物同时支持 Intel 与 Apple Silicon。
- 在 Windows runner 上构建 x64 `.msi` 与 NSIS `.exe` 安装包。
- 发版前同步项目版本文件；打包时 workflow 还会用 tag 去掉 `v` 的部分覆盖 Tauri 构建版本（例如下一个补丁版本 `v0.4.1` → `0.4.1`）。
- 两个平台分别通过前端测试、Rust 测试、fmt、Clippy 和生产构建后，才会创建 Release。
- 带 `-rc`/`-beta` 等后缀的 tag 自动创建预发布；不带后缀的 tag 创建正式 Release。
- Release 上传 macOS `.dmg` / `.app.tar.gz`、Windows `.msi` / `.exe`，以及 updater 必需的签名文件和 `latest.json`；GitHub 自动附带的 `Source code` 压缩包由平台生成，无法通过 workflow 关闭。

### 软件检测更新

Luma 已接入 Tauri updater：启动时会静默检查 GitHub Releases，发现新版本后在标题栏显示版本号。用户点击更新按钮后，应用会下载签名产物、显示进度、安装并重启；检查失败可以手动重试，不会静默强制升级。

- 更新源：`https://github.com/you-want/Luma/releases/latest/download/latest.json`
- 更新清单引用带 minisign 签名的各平台安装包；客户端按内置公钥校验产物，签名不匹配的更新会被拒绝。清单本身不单独签名。
- 发布 workflow 需要配置 GitHub Actions secret：`TAURI_SIGNING_PRIVATE_KEY`；如果生成密钥时设置了口令，还需配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- 发布构建从 GitHub Secret 读取私钥，本机另有受限权限的备份；私钥与口令绝不提交到仓库，仓库内只保存 updater 公钥。
- 这不是 Apple Developer ID 或 Windows 代码签名。当前安装包仍可能出现系统来源未验证提示，代码签名、公证和商店分发仍未启用。

密钥配置、发布产物检查和升级验收记录见 [`docs/updater-release.md`](docs/updater-release.md)。

## 反馈与支持

遇到问题、有功能建议或想讨论使用场景？

- **报告 Bug 或提出功能需求** — [GitHub Issues](https://github.com/you-want/Luma/issues)
- **使用讨论与问答** — [GitHub Discussions](https://github.com/you-want/Luma/discussions)

贡献代码或改进文档前，建议先开 Issue 讨论方向。

## 当前限制

- **平台支持**：macOS universal 构建已验证。Windows 11 x64 代码适配、安装包和 updater 产物 workflow 已完成，但尚未在 Windows 真机回归；安装、启动、卸载、扫描和更新烟测待 Windows 环境。Linux 暂不在路线图。
- 不提供全盘后台扫描、自动监控、永久删除或未经确认的后台整理。
- 默认扫描不读取文件内容；用户主动执行重复检测时，会读取候选文件内容计算本地哈希。
- “开发构建内容”使用路径名称规则（`node_modules`、`target`、`dist`、`.next`），不判断对应内容是否仍被项目需要。
- 扫描统计代表扫描时读取到的快照；扫描期间发生的文件变化可能导致少量误差并计入错误数。
- Apple/Windows 代码签名与公证当前仍未启用；应用内更新使用独立的 Tauri updater 签名校验，已确认一次 `v0.3.2 → v0.4.0` 应用内升级成功，完整设备矩阵及失败场景仍待验收。

## 手工回归清单

发布内部构建前，使用一个包含空目录、隐藏文件、符号链接、大文件和无权限目录的测试夹验证：

1. 首次启动、选择目录并完成扫描。
2. 扫描中取消，确认最近成功结果没有被覆盖。
3. 连续扫描两个目录，确认进度和结果没有串线。
4. 关闭并重启，确认恢复最近成功结果。
5. 从最大文件列表在 Finder 中显示文件。
6. 展开每一类“值得留意”发现，确认明细文件与汇总数字一致，并能在 Finder 中定位。
7. 确认被扫描目录的内容和时间戳未被 Luma 修改。

## 官网维护

官网是 `website/` 下的静态页面，生产地址为 <https://luma.raingpt.top/>，由 Vercel 跟随 `main` 自动部署。版本更新时同步首页版本、功能说明、平台下载链接、隐私联网说明和路线图；提交后验证生产域名已显示新内容。

本地预览：`python3 -m http.server 1031 --directory website`。官网源码检查、链接与部署状态应与应用测试分开记录；修改纯文档或官网内容不需要重新发布桌面安装包。
