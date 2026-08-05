# Luma

Luma 是一个面向 macOS 的本地文件空间观察工具。它只读扫描用户明确选择的目录，在本机建立 SQLite 索引，并展示空间分类、最大文件与可解释的复查线索——全程不读取文件内容，不联网，不修改任何原文件。

当前版本：`0.1.0`（内部 MVP）

## 能做什么

- **只读目录扫描** — 选择单个目录递归扫描，默认忽略隐藏项且不跟随符号链接；可选是否包含隐藏文件、是否跨文件系统。
- **实时进度与取消** — 实时展示文件数、目录数、已读取大小、当前路径与错误数（约每 100 毫秒节流一次）。可随时取消；单个文件无权限、消失或元数据损坏只计入错误数，不中断整个任务。
- **空间概览** — 按分类展示占用的分段条（图片、视频、音频、文档、压缩包、代码、应用、其它），风格接近系统设置里的存储视图。
- **最大文件** — 列出前 20 个最大文件，可在 Finder 中定位。
- **“值得留意”发现与明细下钻** — 按可调阈值发现五类项目：超大文件、长期未修改、开发构建内容、压缩包、应用与安装包。每一类都能点开查看其中最大的前 10 个文件并在 Finder 中定位。明细列表与汇总数字复用同一套判定规则，两者始终一致。
- **本地持久化** — 结果保留在本机 SQLite，最近 3 次终态扫描滚动保留；启动后自动恢复最近一次成功结果。完整文件索引不进入前端状态。

## 安全与隐私

Luma 的扫描过程只读取文件路径和元数据（名称、扩展名、大小、修改时间），不会读取文件内容，也不会修改、移动或删除被扫描的文件。扫描结果不上传，应用不进行任何联网调用。

应用只写入自己的 Tauri 应用数据目录。macOS 上数据库通常位于：

```text
~/Library/Application Support/com.rain9.luma/luma.sqlite3
```

“大文件”“长期未修改”等发现只表示符合透明规则，不能据此判断文件可以安全删除。修改时间也不等于最近使用时间。

## 技术栈

- **前端** — React 19 + TypeScript + Vite，`lucide-react` 图标；轻量的 `useSyncExternalStore` store，无第三方状态库。
- **后端** — Rust + Tauri 2；`walkdir` 遍历、`rusqlite`（bundled SQLite）持久化、`uuid` 生成扫描 ID。
- **进程模型** — 扫描在 Rust 阻塞任务中执行，文件分批（每批 500 条）写入 SQLite，进度通过事件回传前端。前端经 `src/lib/tauri.ts` 统一调用后端命令。

## 项目结构

```text
src/                    前端
  app/store.ts          全局状态（useSyncExternalStore）
  components/           UI 组件（扫描控制、进度、概览、分类、最大文件、发现）
  hooks/useScanEvents   订阅后端扫描进度/完成事件
  lib/tauri.ts          后端命令封装
  lib/format.ts         字节/数字/日期格式化
  types/scan.ts         前后端共享类型
src-tauri/src/          后端
  scanner.rs            目录遍历与文件分类
  database.rs           SQLite schema、写入与查询
  commands.rs           Tauri 命令入口
  models.rs             序列化数据模型
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

产物位于 `src-tauri/target/release/bundle/`（`.app` 与 `.dmg`）。

### 打 tag 自动发版

`.github/workflows/release.yml` 会在推送 `v*` 形式的 tag 时自动打包并发布到 GitHub Releases：

```bash
git tag v0.2.0
git push origin v0.2.0
```

流程说明：

- 在 macOS runner 上构建 `universal-apple-darwin` 通用二进制，一份产物同时支持 Intel 与 Apple Silicon。
- 打包前自动把应用版本同步为 tag 去掉 `v` 的部分（`v0.2.0` → `0.2.0`），无需手动改 `tauri.conf.json` 再提交。
- 发版前先跑前端与 Rust 测试，测试失败则不发布。
- 用该 tag 创建 Release 并上传 `.dmg` / `.app`，附带未签名应用的首次打开提示。

不额外配置任何 secret，仅使用 Actions 自带的 `GITHUB_TOKEN`。

### 签名与公证

面向公开分发前还需要 Apple 代码签名与公证，否则 macOS Gatekeeper 会拦截未签名的应用：

1. 使用 Apple Developer 证书对 `.app` 进行代码签名（Developer ID Application）。
2. 提交公证（notarization）并 staple 结果到 `.dmg`。
3. 在其它 Mac 上验证首次打开不再出现“无法验证开发者”的警告。

当前构建尚未签名和公证，只面向内部构建验证。

## 反馈与支持

遇到问题、有功能建议或想讨论使用场景？

- **报告 Bug 或提出功能需求** — [GitHub Issues](https://github.com/you-want/Luma/issues)
- **使用讨论与问答** — [GitHub Discussions](https://github.com/you-want/Luma/discussions)

贡献代码或改进文档前，建议先开 Issue 讨论方向。

## 当前限制

- 首个验收平台是 macOS，尚未承诺 Windows 或 Linux 兼容性。
- 不提供全盘后台扫描、自动监控、删除、移动或归档能力。
- 不读取文件内容，因此暂不识别内容重复文件。
- “开发构建内容”使用路径名称规则（`node_modules`、`target`、`dist`、`.next`），不判断对应内容是否仍被项目需要。
- 扫描统计代表扫描时读取到的快照；扫描期间发生的文件变化可能导致少量误差并计入错误数。
- 应用尚未进行公证和发布签名，只面向内部构建验证。

## 手工回归清单

发布内部构建前，使用一个包含空目录、隐藏文件、符号链接、大文件和无权限目录的测试夹验证：

1. 首次启动、选择目录并完成扫描。
2. 扫描中取消，确认最近成功结果没有被覆盖。
3. 连续扫描两个目录，确认进度和结果没有串线。
4. 关闭并重启，确认恢复最近成功结果。
5. 从最大文件列表在 Finder 中显示文件。
6. 展开每一类“值得留意”发现，确认明细文件与汇总数字一致，并能在 Finder 中定位。
7. 确认被扫描目录的内容和时间戳未被 Luma 修改。
