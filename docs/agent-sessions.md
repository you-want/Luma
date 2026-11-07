# Agent 会话（Agent Sessions）工作包

> 状态基线：v0.5.0 代码封板（2026-11-07）
> 关联文档：[磁盘管家](disk-cleaner.md)、[产品路线图](product-roadmap.md)

## 1. 目标与定位

AI Agent CLI（Claude Code / Codex / Gemini CLI）会把完整会话转录写到本机用户目录下，随使用量持续增长且难以手动管理。参照 cc-switch 的「会话」页，Luma 新增一个独立视图：

> **看清有哪些会话 → 按项目/时间/应用筛选 → 批量移至废纸篓**

与 Luma 既有红线完全一致：**只移至废纸篓、永不永久删除、删除边界重验、写操作必须显式确认**。本功能**不扫描正文内容入库**，只提取元数据（标题、首末消息预览、时间、大小），全部计算在本地完成。

## 2. 数据源（本机实测验证）

| Agent | 数据目录 | 格式 | 标题来源（优先级从高到低） |
|---|---|---|---|
| Claude Code | `~/.claude/projects/<编码cwd>/<uuid>.jsonl` | JSONL | `custom-title` → `ai-title` → 首条真实用户消息 → 目录名 |
| Codex CLI | `~/.codex/sessions/**/rollout-*.jsonl`（含 `archived_sessions/`） | JSONL | `session_index.jsonl` 的 `thread_name` → 首条非注入用户消息 → 目录名 |
| Gemini CLI | `~/.gemini/tmp/<key>/chats/session-*.json(l)` | JSON / JSONL 回放 | 首条非 `/` 命令用户消息 → 项目名 |

解析要点（对齐 cc-switch 的经验）：

- **只读头尾窗口**：每个转录只读前 12 行 + 末 40 行（末尾最多回读 256 KiB），大文件（几十 MB）列表依然瞬时。
- **cwd 取行内字段**：Claude 取记录里的 `cwd`，Codex 取 `session_meta.payload.cwd`，Gemini 读 `<key>/.project_root` 标记文件——不做目录名反解（有损）。
- **跳过注入文本**：`<command-name>`、`<local-command-caveat>`、`<system-reminder>`（Claude）、`<environment_context>`、`# AGENTS.md`（Codex，并从 `## My request for Codex:` 之后取真实请求）、`/` 命令与 `<session_context>`（Gemini）。
- **跳过子代理转录**：Claude 的 `agent-*.jsonl` / `journal.jsonl` 不算独立会话；`.jsonl` 与 `.json` 孪生（Gemini）只计一次。
- **解析防御性**：未知行类型一律跳过，坏行不影响其余数据。
- **mtime + 大小缓存**：文件未变时复用上次解析结果，重复刷新不重读。

## 3. 实现结构

- 后端：`src-tauri/src/sessions.rs`
  - `PROVIDERS` 注册表：每家 Agent = id + `$HOME` 相对 roots + 文件名匹配器 + 解析函数。**新增 Agent 只加一个条目**。
  - `discover_sessions()` → `AgentSessionsReport { agents, sessions }`（camelCase serde，按最近活跃排序）。
  - `trash_sessions(db_path, items)` → `CleanOutcome`：重跑发现后逐项校验（路径必须在新鲜发现结果中、大小一致、`validate_write_path`、白名单 fail-closed），主文件走 `file_ops::trash_one`（Finder → 废纸篓），操作写入 `file_operations` 表（`scan_id = 'cleaner'` 哨兵，同磁盘管家）。
- 命令：`discover_agent_sessions` / `trash_agent_sessions`（`commands.rs`，注册于 `lib.rs`）。
- 前端：`src/components/sessions/SessionsPanel.tsx` + `src/types/sessions.ts` + `src/lib/tauri.ts` 包装。
  - 工具栏：Agent 下拉（含计数）、搜索（标题/目录/首末消息/会话 ID）、「按时间 / 按项目」分段切换、刷新。
  - 分组列表：按项目（项目名 + 路径 + 会话数 + 总大小，可折叠）或按时间（今天/昨天/近 7 天/近 30 天/更早）。
  - 会话行：复选框、标题、最近消息预览、最近活跃（相对时间）、大小、复制恢复命令（`claude --resume <id>` 等，在项目目录执行）、单条删除。
  - 批量条：全选（作用于当前筛选结果）+ 批量删除 → `ConfirmDialog`（danger）→ 结果横幅 → 自动刷新。

## 4. 删除语义（逐 Agent 附属清理）

删除 = **转录 + 附属数据一起进废纸篓**，防"删了又复活"：

| Agent | 一并移至废纸篓的附属 |
|---|---|
| Claude Code | 同目录 `<session-id>/`（子代理转录、工具输出）、`~/.claude/file-history/<id>/`、`~/.claude/session-env/<id>/`、`~/.claude/todos/<id>*.json` |
| Codex CLI | 同名 `.jsonl.zst` 压缩孪生（只删一个会从另一个复活） |
| Gemini CLI | 另一个扩展名的旧孪生（`.json` ⇄ `.jsonl`） |

安全层：路径必须落在该 Agent 注册 roots 下且通过新鲜发现索引匹配；`FILE_CHANGED`（大小不符）、`PATH_NOT_IN_SCOPE`、`WHITELISTED_PATH`、`PROTECTED_PATH` 均逐项报告失败原因，不中断批次。

**明确不做**（v1）：会话正文预览、终端内恢复会话（仅提供复制恢复命令）、Codex `session_index.jsonl`/`state_5.sqlite` 行清理（删文件后残留的标题条目无害）、Claude `history.jsonl` 行清理。

## 5. 扩展新 Agent 的步骤

1. 在 `sessions.rs` 的 `PROVIDERS` 加一个条目：roots（`$HOME` 相对）、文件名匹配器、解析函数。
2. 解析函数返回 `AgentSession`（复用 `build_session` / `message_texts` / `parse_rfc3339_secs` 等既有助手）；有外部标题索引就在 `Ctx` 里加一个预载 map。
3. 如有附属数据，在 `sidecar_paths()` 加一个分支。
4. `src/types/sessions.ts` 的 `AgentId` 联合类型 + `AGENT_META`（图标 + `sessions.agentNames.*` i18n 键，zh-CN/en-US 各一行）。
5. 补 fixture 单元测试（参考 `parse_claude_extracts_metadata` 等）。

## 6. 验证

- Rust：`cargo test sessions::`（22 个：时间戳/文本提取/三家解析 fixture/子代理跳过/附属收集/发现分组/删除校验与白名单 fail-closed）。
- 前端：`pnpm build`（tsc 类型检查含 i18n 键漂移守卫）+ `pnpm test`。
- 手动：会话页应展示本机真实会话（本机实测 Claude 26 / Codex 73 / Gemini 14，与 cc-switch 计数一致）；删除单条会话后废纸篓出现转录文件，列表刷新后消失。
