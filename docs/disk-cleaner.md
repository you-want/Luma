# 磁盘管家（Disk Cleaner）工作包

> 状态基线：v0.5.0 代码封板（2026-11-07）
> 关联文档：[产品路线图](product-roadmap.md)、[本地存储说明](local-storage.md)

## 1. 目标与定位

用户的目标很直接：**电脑莫名其妙被占满了，想知道是什么占的、哪些可以删。**

Luma 现有能力从"选定目录"出发（扫描 → 分析 → 整理），磁盘管家把它补全为整台电脑的视角：

> **看清占用 → 找到可删 → 安全清理**

与 CLI 清理工具的定位差异：Luma 是 GUI，坚持既有红线——**只移至废纸篓、永不永久删除、一切可解释、写操作必须显式确认**。这一立场自 v0.5.0 起成为官方产品定位：“保守写”（见 [产品路线图](product-roadmap.md) 的产品立场修订），对外文案须与此口径一致。

## 2. 设计取舍

| 常见的 CLI 清理工具做法 | Luma 的落地 | 状态 |
|---|---|---|
| 缓存/日志/残留清理 | M2 清理中心（固定位置注册表 + 风险分级） | v0.5.0 |
| 安装包发现 | 清理中心 installers 类（Downloads/Desktop 顶层安装镜像） | v0.5.0 |
| 按项目分组的可重建产物，7 天活跃跳过 | M3 项目瘦身 | 后续 |
| 应用 + 残留清理，共享数据保留 | M4 应用残留清理 | 后续 |
| `--dry-run` / 预览先行 | 全线预览文件清单，确认后才执行 | v0.5.0 |
| `--whitelist` 持久化保护 | SQLite 白名单，列表与执行双层过滤 | v0.5.0 |
| 白名单读不到就拒绝执行（fail closed） | 同样：白名单加载失败 → 拒绝清理 | v0.5.0 |
| 删除边界重验文件身份 | 执行前复核存在性/类型/大小/修改时间 | v0.5.0 |
| 删除走废纸篓 | 比它更保守：清理中心**没有**永久删除，废纸篓本身也只列出不清空 | v0.5.0 |
| 操作日志 operations.log | 复用 `file_operations` 表 | v0.5.0 |
| 跳过原因明示（"pnpm busy"） | 白名单/范围外/平台不可用均有明确原因 | v0.5.0 |
| 时间预算防扫描过久 | 每类别统计预算 2 秒，超时标记 `isPartial`（"至少"） | v0.5.0 |

**明确不做**：系统数据库优化等侵入性维护（远期再议）、二进制瘦身（远期再议）、系统监控 watch、TUI 交互、清空废纸篓（属永久删除，超出产品红线）。

## 3. v0.5.0 范围（磁盘管家 I）

### CLN-001 位置注册表与统计

- Rust `cleaner.rs` 维护一张固定位置注册表，每个类别 = id、风险级、收集模式、平台根路径：
  - `userCaches`（safe）：`~/Library/Caches` / `%LOCALAPPDATA%\Temp`
  - `appLogs`（safe）：`~/Library/Logs` / `%LOCALAPPDATA%\CrashDumps`
  - `browserCaches`（caution）：Safari / Chrome / Firefox 缓存（Windows：Chrome / Edge）
  - `pkgManagerCaches`（safe）：npm / pnpm / pip / uv 缓存（Windows：npm / pip）
  - `devDerived`（safe，仅 macOS）：Xcode DerivedData、模拟器缓存
  - `installers`（review）：Downloads/Desktop 顶层 `dmg/pkg/mpkg/iso/xip`
  - `oldDownloads`（review）：下载目录超过 180 天未修改的文件（递归，与现有清理建议一致）
  - `trash`（review）：废纸篓（**只列出，不执行**）
- 统计带每类别时间预算，超时 `is_partial: true`；不存在的位置直接不展示。

### CLN-002 白名单

- SQLite schema v6 新增 `cleaner_whitelist(path PRIMARY KEY, created_at)`。
- 白名单路径从列表和执行两个层面过滤；缓存类子项与白名单路径**双向重叠**时一律跳过（子项内部包含受保护路径也不删）。
- fail closed：白名单加载失败时，总览、明细、执行全部报错拒绝，绝不裸跑。

### CLN-003 清理执行

- 前端只发送用户看到并勾选的 `{path, sizeBytes}` 清单；后端逐项复核：
  1. 绝对路径、无上级跳转、不在系统保护路径（复用 `validate_write_path`）；
  2. 必须位于该类别的注册根路径之内（拒绝任意路径注入）；缓存类必须是一级子项；
  3. 存在且类型一致；单文件大小与列表一致；安装包扩展名、旧下载时间重新校验；
  4. 不与白名单重叠。
- 删除一律走系统废纸篓（复用 `trash_one`：macOS osascript / Windows 回收站）。
- 写入 `file_operations`（`scan_id = 'cleaner'`，kind `trash`，无撤销载荷——废纸篓就是恢复途径）。
- 失败逐项收集，不中断批次；结果含成功/失败明细。

### CLN-004 磁盘容量总览

- macOS 解析 `df -k`（`/` 与 `/Volumes/*`，排除快照卷）；Windows 解析 `Get-PSDrive` 输出。
- 只读、尽力而为：查询失败返回空列表，前端隐藏该区块，不影响清理功能。

### CLN-005 前端磁盘管家页

- 顶栏第三个页签「磁盘管家」，**无需先扫描目录**即可使用。
- 磁盘容量条 + 类别卡片（风险徽章、占用、文件数、依据说明）+ 展开明细（勾选）+ 确认执行 + 结果反馈。
- 白名单管理：查看、从系统目录选择框添加、移除。

### CLN-006 国际化

- 全部文案进入 `cleaner.*` 命名空间，中英键集合一致测试通过；错误码 `INVALID_CLEAN_KIND`、`WHITELIST_UNAVAILABLE`、`PATH_NOT_IN_SCOPE`、`WHITELISTED_PATH` 进 `errors.*`。

## 4. 安全红线（沿用产品既有承诺）

- 只移至废纸篓；没有永久删除；废纸篓只统计不清空。
- 只处理注册表中的固定位置；拒绝注册范围之外的任何路径。
- 执行前复核；白名单 fail closed；操作有日志。
- 不要求 sudo/管理员权限；不读取文件内容。

## 5. 验收标准

- 前端测试、生产构建、Rust test / fmt / Clippy 全部通过。
- 注册表类别 id 唯一、风险分级齐备；每条路径规则有单元测试。
- 白名单 CRUD、前缀边界（`/foo/bar` 不匹配 `/foo/baz`）、双向重叠、fail closed 有测试。
- 执行校验：范围外路径 / 白名单路径 / 大小变化 / 已删除文件均被拒绝且不中断其余项。
- 执行测试使用注入的删除器，绝不触碰真实废纸篓。
- 中英文文案键集合一致。

## 6. 后续里程碑

| 里程碑 | 内容 | 任务段 | 状态 |
|---|---|---|---|
| 磁盘管家 II | M3 项目瘦身：按项目分组、7 天活跃保护、git 感知 | CLN-101 / CLN-102 | `DONE`（2026-11-04） |
| 磁盘管家 III | M4 应用残留清理：孤儿残留发现、风险分级、个人数据默认保留 | CLN-201 / CLN-202 | `DONE`（2026-11-04，macOS） |
| 磁盘管家 IV | 模拟器设备清理、清理历史、健康摘要 | CLN-301 / CLN-303 / CLN-304 | `DONE`（2026-11-04） |
| 磁盘管家 V | 通用二进制瘦身、系统维护、系统状态 | CLN-302 / CLN-401 / CLN-402 | `DONE`（2026-11-04） |
| 应用残留清理 Windows 版 | 注册表卸载模型 | CLN-203 | `TODO`（阻塞于无 Windows 真机，与 WIN-009 同因） |

### CLN-302 通用二进制瘦身（磁盘管家 V，macOS / Apple Silicon）

新增 Rust 模块 `slim.rs`，在 Apple Silicon 上剥离通用应用中无用的 Intel 代码（通常约占应用体积一半）：

- **候选门槛**：仅 Apple Silicon Mac（x86_64 主机直接报告不支持）；主可执行文件必须同时含 arm64 与 x86_64 切片；App Store 应用（存在 `_MASReceipt`）与 Apple 自有应用（`com.apple.*`）一律排除；白名单路径过滤照常生效。
- **预计可省 = 包内所有 fat Mach-O 的 x86_64 切片大小之和**（`lipo -detailed_info` 逐文件解析），诚实口径。
- **全程副本操作**：`ditto` 复制 → 逐文件 `lipo -thin arm64` → `codesign --force --deep --sign -` 临时重签 → `codesign --verify --strict` 必须通过 → 原子换入（原应用改名暂存 → 瘦身副本就位 → 原件才移入废纸篓）。任何一步失败即中止并清理副本，**原应用毫发无损**。
- **已知代价（UI 中明示）**：临时签名替换了 Developer ID 签名（首次访问钥匙串可能重新授权、丢失部分 entitlements——对本机运行一般无害）；若应用行为异常，从废纸篓恢复原件即可。

### CLN-401 系统维护（磁盘管家 V）

只实现与磁盘相关且可诚实执行的两项，写入 `file_operations`（kind `maintenance`）并出现在清理历史：

- **QuickLook 缓存刷新**（`qlmanage -r cache`，无需授权）：清空预览缩略图缓存，回收其占用并修复预览异常。
- **DNS 缓存刷新**：通过 `osascript … with administrator privileges` 执行——系统弹出原生授权窗口（配置了触控 ID 的 Mac 可用触控 ID 确认）。一次性特权任务走系统授权，不引入常驻特权助手。
- 系统数据库优化等侵入性维护不采纳；任务表为封闭枚举，未知任务拒绝执行。

### CLN-402 系统状态卡片（磁盘管家 V）

- 磁盘管家页顶部卡片：CPU 使用率、内存占用（进度条 + 数值）、电池状态（百分比 + 是否接电）。
- **一次一个快照，不做后台 watch**（产品红线：不在后台运行）；挂载时加载 + 8 秒间隔刷新，随时手动刷新。
- 零新依赖：macOS 解析 `top -l 1`（CPU usage / PhysMem 行）与 `pmset -g batt`，Windows 解析 PowerShell CIM 查询，解析器全部纯函数并带单元测试；工具失败逐字段降级为"暂不可读"。

### 明确不做的两项（无对应场景）

- **TUI 交互**：终端交互范式，GUI 应用的对应物就是现有的图形交互本身，不存在字面移植。
- **Touch ID sudo**：CLI 的特权授权机制；Luma 的对应实现即 CLN-401 的系统授权窗口（macOS 原生弹窗在支持触控 ID 的设备上自动提供触控 ID 选项）。

### CLN-301 失效模拟器设备清理（磁盘管家 IV）

- 新增清理类别 `simDevices`（macOS）：候选**不来自目录遍历**，而是 `xcrun simctl list devices -j`——只接受系统明确报告 `isAvailable: false` 的设备（运行时已删、无法启动），`dataPath` 反推设备目录为删除单元，名称显示为"设备名 (运行时)"。
- **valid 设备永远不会成为候选**：缺 `isAvailable` 标志、缺 `dataPath`、目录不存在的条目一律跳过（fail closed）；执行前重新运行 simctl 并要求路径 + 大小完全一致（`KindDef::reverify` 机制，防止把在用设备传进执行器）。
- `devDerived` 类别补入 `~/Library/Developer/Xcode/iOS DeviceSupport`（重连设备即重建）。

### CLN-303 清理历史（磁盘管家 IV）

- 复用 `file_operations`（`scan_id='cleaner'`）做只读查询：`list_cleaner_operations` 按时间倒序返回最近 20 条，覆盖磁盘管家清理 / 项目瘦身清理 / 应用残留清理三类记录。
- 前端磁盘管家页新增「清理历史」折叠区：状态点（已完成/部分完成/失败/进行中/已中断）+ 操作摘要 + 时间。

### CLN-304 磁盘健康摘要（磁盘管家 IV）

- 磁盘管家页顶部一行：「本机可回收约 X GB · 基于 N 个位置的评估」——聚合现有总览数据，不新增扫描。

### 后续候选（未启动）

- **CLN-203 应用残留清理 Windows 版**（CLN-202c）：仍延后——应用卸载与残留依赖注册表模型（卸载器、MSI、服务），文件系统近似会误报，且无 Windows 真机可验收（与 WIN-009 同因阻塞）。
- CLN-302 二进制瘦身已于 2026-11-04 采纳并实现（见上方 CLN-302 条目），不再待产品确认。

### CLN-101 项目瘦身（磁盘管家 II）

新增 Rust 模块 `purge.rs`，按 Luma 红线收紧：

- **发现范围受限**：只扫描家目录下的注册开发目录（Projects / Developer / Code / dev / workspace / src / Documents / Desktop），项目目录最多在根下 3 层，产物目录最多在项目内 2 层（兼容 monorepo）。总预算 10 秒，按需触发（用户点开才扫描），超时标记部分结果。
- **标志文件守门**（保守判定）：目录只有在项目直接包含对应标志文件时才算构建产物——`package.json` → `node_modules` / `.next` / `dist` / `build`，`Cargo.toml` → `target`，`pom.xml` → `target`，`build.gradle` → `build` / `.gradle`，`pyproject.toml` / `requirements.txt` / `setup.py` → `.venv` / `venv`。碰巧叫 `target` 的普通资料目录永远不会被碰。
- **7 天活跃保护**：项目内任何文件 7 天内被修改 → 标记「活跃中」，产物在 UI 中锁定，执行器即使收到过期请求也拒绝（`PROJECT_ACTIVE`）。
- **删除边界重发现复核**：执行时重新跑一遍发现，请求中的产物路径必须出现在新结果中且大小一致，否则拒绝（`FILE_CHANGED` / `PATH_NOT_IN_SCOPE`）；白名单双向重叠与系统保护路径照常拦截。
- 全部移至废纸篓，`file_operations` 记录（label「项目瘦身清理」）；每个产物组显示重建提示（如 "npm install 可恢复"）。
- 已知边界：monorepo 根与子包的产物大小存在重叠（展示口径，执行按产物路径互不影响）；Xcode DerivedData 不在本模块（磁盘管家 I 的 devDerived 类已覆盖）。

### CLN-102 git 感知（磁盘管家 II 增强）

- 项目根存在 `.git`（目录或 worktree 文件）时，对每个产物运行 `git ls-files`（限项目内相对路径）：有输出即视为被跟踪。
- **被跟踪的产物不参与清理**：UI 显示「已被 git 跟踪」徽章并禁用勾选，执行器收到过期请求也拒绝（`GIT_TRACKED`）。删除被跟踪产物会弄脏工作区，且可能是有意提交的内容。
- **fail closed**：git 命令缺失或仓库损坏时一律视为被跟踪（宁可不清理，不在无法审查的仓库里提供删除）。

### CLN-202 孤儿残留增强（磁盘管家 III 增强，macOS）

- **CLN-202a 启动代理**：扫描 `~/Library/LaunchAgents/*.plist`（只扫用户级，系统 launchd 配置永不触碰）。候选条件：plist 名为 bundle-id 样式、该 id 无已装应用、且**内容复核**通过——经 `plutil` 规范化为 XML 后提取所有 `<string>` 值，若有任何引用指向仍存在的 .app 包则视为"服务于已装应用"跳过；无法读取的 plist 一律跳过（保守）。风险级 review，默认不勾选。
- **CLN-202b 共享容器归属**：`Group Containers` 中 `group.<bundle-id>` 样式的目录，按去掉 `group.` 前缀后的 bundle id 做已装反查（同其他组规则）；Apple `TEAMID.group.…` 样式无法可靠归属，一律跳过。风险级 review。
- **CLN-202c Windows 版**：仍延后——应用卸载与残留依赖注册表模型（卸载器、MSI、服务），文件系统近似会误报，且无 Windows 真机可验收（与 WIN-009 同因阻塞）。

### CLN-201 孤儿应用残留清理（磁盘管家 III）

新增 Rust 模块 `orphans.rs`，收窄为**孤儿残留清理**（macOS）：

- **应用本体永不触碰**：只有当 bundle identifier 不被任何已安装 .app 声明时（扫描 /Applications、~/Applications、/System/Applications 的 Info.plist，经系统 `plutil` 读取）才成为候选——在用应用和系统应用的数据永远不会出现。
- **只认 bundle-id 样式标识**（`com.company.app`，段字母开头、点分、无空格）：Application Support 里普通名字的目录无法可靠归属，一期不列，零误报优先。`group.*` 共享容器（多应用共用）一律跳过。
- **扫描位置**：Caches、Saved Application State、WebKit、HTTPStorages（safe，默认勾选）；Application Support、Preferences（.plist）、Containers、Logs（review，默认不勾选——个人数据默认保留）。
- **边界与后续**：Group Containers（共享数据）与 LaunchAgents（登录项）不做，列 CLN-202；Windows 依赖注册表卸载模型，报告 `platformSupported: false`，前端明示"仅 macOS 可用"；从 DMG 直接运行（未安装）的应用其残留可能被列入，已通过 review 类默认不勾选缓解。
- 执行前重发现复核（路径 + 大小）、白名单双向重叠、系统保护路径拦截、废纸篓路由、`file_operations` 日志（label「应用残留清理」），与磁盘管家 I/II 同一安全管线。
- 8 秒预算，按需触发（用户点开才扫描），超时标记部分结果。
