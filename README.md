# Luma

Luma 是一个本地文件空间观察工具，当前已验证 macOS 平台，Windows 11 x64 代码适配已完成。它扫描用户明确选择的目录，在本机建立 SQLite 索引，并展示空间分类、最大文件、重复文件候选、开发项目与可解释的复查线索。默认扫描只读取路径和元数据；用户主动点击”查找重复”后，Luma 才会读取候选文件内容并计算本地 BLAKE3 哈希，全程不上传数据、不修改原文件。

当前版本：`0.2.0`（内部预览）

下一阶段发布候选目标：`v0.4.0-rc.1`。该候选版本将同时验收 macOS universal 与 Windows 11 x64 安装包。

后续功能与逐项进度见 [`docs/product-roadmap.md`](docs/product-roadmap.md)。

## 能做什么

- **只读目录扫描** — 选择单个目录递归扫描，默认忽略隐藏项且不跟随符号链接；可选是否包含隐藏文件、是否跨文件系统。
- **实时进度与取消** — 实时展示文件数、目录数、已读取大小、当前路径与错误数（约每 100 毫秒节流一次）。可随时取消；单个文件无权限、消失或元数据损坏只计入错误数，不中断整个任务。
- **空间概览** — 按分类展示占用的分段条（图片、视频、音频、文档、压缩包、代码、应用、其它），风格接近系统设置里的存储视图。
- **搜索与过滤** — 按文件名或路径搜索已索引文件，可按分类、扩展名、大小范围、修改时间过滤，支持排序与分页；搜索结果可在 Finder 中定位。
- **批量选择与非破坏性操作** — 在搜索结果中按项或跨分页选择文件，支持全选当前页与「全部筛选结果」两种语义；首期提供复制路径、导出清单（CSV）等非破坏性操作，不移动、不删除任何原文件。
- **最大文件** — 列出前 20 个最大文件，可在 Finder 中定位。
- **”值得留意”发现与明细下钻** — 按可调阈值发现五类项目：超大文件、长期未修改、开发构建内容、压缩包、应用与安装包。每一类都能点开查看其中最大的前 10 个文件并在 Finder 中定位。明细列表与汇总数字复用同一套判定规则，两者始终一致。
- **重复文件候选** — 先按文件大小筛选，再对候选文件计算 BLAKE3 内容哈希，展示重复组、可节省空间和 Finder 定位入口。只有用户主动执行查找时才读取文件内容。
- **开发项目识别** — 识别 Node.js、Rust、Python、Git、Xcode、Maven 和 Gradle 项目，展示项目类型、文件数、占用空间与路径。
- **扫描历史对比** — 按目录保留最近 3 次终态扫描，可比较总容量、文件数量和分类变化。
- **本地持久化** — 结果保留在本机 SQLite，最近 3 次终态扫描滚动保留；启动后自动恢复最近一次成功结果。完整文件索引不进入前端状态。
- **中英文界面** — 内置简体中文与英文，首次启动按系统语言自动选择，可在标题栏手动切换并持久化保存；数字、容量与日期随语言本地化。

## 安全与隐私

Luma 的普通扫描只读取文件路径和元数据（名称、扩展名、大小、修改时间），不会修改、移动或删除被扫描的文件。重复检测是显式的用户操作，会读取满足大小条件的候选文件内容，仅在本机计算 BLAKE3 哈希；文件内容和哈希都不会上传。

应用只写入自己的 Tauri 应用数据目录。macOS 上数据库通常位于：

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

### 打 tag 自动发版

`.github/workflows/release.yml` 会在推送 `v*` 形式的 tag 时，在 macOS 与 Windows runner 分别打包并发布到同一个 GitHub Release：

```bash
git tag v0.4.0-rc.1
git push origin v0.4.0-rc.1
```

流程说明：

- 在 macOS runner 上构建 `universal-apple-darwin` 通用二进制，一份产物同时支持 Intel 与 Apple Silicon。
- 在 Windows runner 上构建 x64 `.msi` 与 NSIS `.exe` 安装包。
- 打包前自动把应用版本同步为 tag 去掉 `v` 的部分（`v0.4.0-rc.1` → `0.4.0-rc.1`），无需手动改 `tauri.conf.json` 再提交。
- 两个平台分别通过前端测试、Rust 测试、fmt、Clippy 和生产构建后，才会创建 Release。
- 带 `-rc`/`-beta` 等后缀的 tag 自动创建预发布；不带后缀的 tag 创建正式 Release。
- Release 上传 macOS `.dmg`/`.app.zip` 与 Windows `.msi`/`.exe`，附带未签名应用的首次打开提示。

### 关于签名与自动更新（当前暂缓）

Luma 当前不发布到 App Store 等官方商店，因此**代码签名、公证与自动更新整体暂缓**：

- 发布流程不生成 updater 清单，应用也不会在后台联网检查版本。
- 构建产物未做 Apple 代码签名/公证，也未做 Windows 代码签名；首次打开时系统可能提示来源未验证，需手动允许。
- 这些能力保留在路线图中（见 [`docs/product-roadmap.md`](docs/product-roadmap.md) 工作包 F，标记为 `SHELVED`），待有官方商店分发或新的分发需求时再重新排期。

## 反馈与支持

遇到问题、有功能建议或想讨论使用场景？

- **报告 Bug 或提出功能需求** — [GitHub Issues](https://github.com/you-want/Luma/issues)
- **使用讨论与问答** — [GitHub Discussions](https://github.com/you-want/Luma/discussions)

贡献代码或改进文档前，建议先开 Issue 讨论方向。

## 当前限制

- **平台支持**：首个验收平台是 macOS。Windows 11 x64 代码适配已完成（路径规范化、隐藏属性、项目识别、reveal 本地化），但尚未在 Windows 真机回归；安装包配置与手工验收待 Windows 环境。Linux 暂不在路线图。
- 不提供全盘后台扫描、自动监控、删除、移动或归档能力。
- 默认扫描不读取文件内容；用户主动执行重复检测时，会读取候选文件内容计算本地哈希。
- “开发构建内容”使用路径名称规则（`node_modules`、`target`、`dist`、`.next`），不判断对应内容是否仍被项目需要。
- 扫描统计代表扫描时读取到的快照；扫描期间发生的文件变化可能导致少量误差并计入错误数。
- 代码签名、公证与自动更新当前暂缓（不发布到官方商店），只面向内部构建验证；详见上文”关于签名与自动更新”。

## 手工回归清单

发布内部构建前，使用一个包含空目录、隐藏文件、符号链接、大文件和无权限目录的测试夹验证：

1. 首次启动、选择目录并完成扫描。
2. 扫描中取消，确认最近成功结果没有被覆盖。
3. 连续扫描两个目录，确认进度和结果没有串线。
4. 关闭并重启，确认恢复最近成功结果。
5. 从最大文件列表在 Finder 中显示文件。
6. 展开每一类“值得留意”发现，确认明细文件与汇总数字一致，并能在 Finder 中定位。
7. 确认被扫描目录的内容和时间戳未被 Luma 修改。
