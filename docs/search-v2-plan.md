# Search v2 执行规划

> 目标：把 Luma 的搜索从“SQLite `LIKE` 查文件名/路径”升级为“本地索引优先、可解释、可扩展的搜索系统”，并为 macOS Spotlight 补充源和后续本地语义搜索预留边界。
>
> 维护规则：每完成一个任务，必须同步勾选任务状态、填写执行记录，并在相关测试通过后再推进下一阶段。

## 1. 产品决策

- **SQLite 是权威数据源**：搜索结果只来自当前扫描快照，和 Luma 的过滤、分页、选择、文件操作安全边界保持一致。
- **Spotlight 是可选补充源**：仅 macOS 启用；Spotlight 未开启、目录被排除或索引延迟时，Luma 仍能正常工作。
- **搜索默认只读**：搜索、索引和结果排序不访问文件内容、不修改文件；任何写操作仍需显式确认。
- **AI 不直接执行文件操作**：未来 AI 只负责把自然语言意图解析成结构化条件，实际结果由确定性的本地查询返回并由用户确认。
- **不把全盘文件复制到云端**：语义搜索优先采用本地文本提取和本地 embedding；云端能力必须是明确的可选项。

## 2. 分阶段范围与验收

| 阶段 | 目标 | 状态 | 完成度 | 验收标准 |
|---|---|---:|---:|---|
| S0 | 现状盘点与边界确定 | `DONE` | 100% | 当前 `LIKE` 搜索、扫描快照、安全边界和 Spotlight 角色已记录 |
| S1 | SQLite FTS5 本地搜索基础 | `DONE` | 100% | 建立可迁移 FTS 索引；扫描写入自动同步；查询仍支持过滤、排序、分页；旧数据库可升级 |
| S2 | 可解释条件搜索 | `DONE` | 100% | 支持 `大于 1GB`、`最近一年`、`PDF`、`代码项目`、`下载目录` 等确定性条件 |
| S3 | 搜索 Provider 抽象 | `IN PROGRESS` | 50% | `LocalSqliteProvider` 已成为默认实现；待加入 macOS provider 和跨 provider 去重 |
| S4 | macOS Spotlight 补充源 | `TODO` | 0% | `NSMetadataQuery` 只在 macOS 构建；失败/未索引时自动回退本地结果；不改变安全模型 |
| S5 | Core Spotlight 应用内索引 | `TODO` | 0% | 可搜索 Luma 自己的扫描历史、项目、洞察和保存的搜索，不索引百万级原始文件记录 |
| S6 | 本地语义搜索（可选） | `TODO` | 0% | 文本提取、embedding、混合召回均可关闭；AI 只能生成结构化查询，不直接删除/移动 |

### S1 任务清单

- [x] S1-001：升级数据库 schema 版本，创建外部内容 FTS5 表。
- [x] S1-002：为 `files` 增加 FTS5 同步触发器，并为已有数据库执行 rebuild。
- [x] S1-003：英文/数字关键词使用 FTS5；中文和特殊字符保留安全的字面量匹配兼容路径。
- [x] S1-004：保留安全的查询词清洗，避免把用户输入当作 FTS 操作符。
- [x] S1-005：增加 FTS 迁移、关键词、中文词、分页、触发器同步和旧数据库升级测试。
- [x] S1-006：记录 10 万文件基线，并检查索引规模。
- [x] S1-007：增加 BM25 字段权重和“相关度”排序，文件名权重高于路径、扩展名和分类。

### S2 任务清单

- [x] S2-001：解析 `大于/超过/>` 与 `小于/低于/<` 容量条件，支持 KB/MB/GB/TB 和小数。
- [x] S2-002：解析 `最近一年`/`近一年` 为确定性的修改时间下限。
- [x] S2-003：解析独立 `PDF` 条件为扩展名过滤；显式筛选器优先于自然语言派生条件。
- [x] S2-004：解析 `下载目录` 为规范化路径条件，兼容 macOS/Windows 的 `/` 入库格式。
- [x] S2-005：解析 `代码项目` 为项目标记文件集合，返回 `package.json`、`Cargo.toml` 等可解释结果。
- [x] S2-006：增加组合条件、中文关键词和项目标记集成测试。

### S3 任务清单

- [x] S3-001：定义 `SearchProvider` trait，并以 `LocalSqliteProvider` 承接当前 Tauri 搜索命令。
- [ ] S3-002：定义多 provider 结果结构、来源标记和 canonical path 去重规则。
- [ ] S3-003：加入 provider 超时、失败隔离和本地结果优先策略。

## 3. 技术设计

### 3.1 本地权威搜索

```text
files（权威扫描快照）
  └── files_fts（FTS5 external-content index）
        ├── name
        ├── path
        ├── extension
        └── category
```

- `files_fts` 使用 `content='files'`、`content_rowid='id'`，避免重复存储完整文件记录。
- `files` 的 insert/update/delete 通过触发器同步 FTS；初始化时执行一次 `rebuild`，确保旧数据库可用。
- FTS 只负责关键词召回；分类、扩展名、大小、时间、隐藏文件仍由 SQLite 条件过滤。
- 查询词转换为安全的 token 前缀表达式；不接受用户传入 `OR`、`NEAR`、列限定等 FTS 语法。

### 3.2 Provider 方向

```text
SearchProvider
  ├── LocalSqliteProvider      # 全平台、权威、默认
  ├── MacSpotlightProvider     # macOS，可选补充
  └── SemanticProvider          # 后续，本地 embedding
```

Spotlight 结果必须经过 canonical path 归一化、当前扫描范围校验和去重；无法证明属于当前扫描快照的结果不能进入批量选择或文件操作链路。

## 4. 执行记录

### 2026-08-17

- 完成 S0：确认现有搜索入口位于 `database::search_files`，当前关键词使用转义后的 `LIKE`；确认扫描快照是安全和产品一致性的基础。
- 完成规划文档初版：确定 SQLite 权威、Spotlight 补充、AI 只做结构化意图解析的边界。
- 完成 S1：schema 升级到 v4；新增 `files_fts`、insert/update/delete 同步触发器、旧库 rebuild、BM25 相关度排序和中英文兼容查询。
- 完成 S2：支持容量、最近一年、PDF、下载目录和代码项目标记等可解释条件；自然语言只生成绑定参数和静态 SQL 条件。
- 开始 S3：新增 `SearchProvider` trait，Tauri 命令默认通过 `LocalSqliteProvider` 查询。
- 10 万文件基线（macOS、本地 debug 测试、仅数据库批量写入与查询）：索引写入 `16.62s`，关键词搜索 `4.52ms`，SQLite 文件 `32,677,888 bytes`（约 `31.2 MiB`）。百万文件基线仍待执行。
- 质量门禁：`pnpm check` 通过（6 项前端测试 + TypeScript/Vite 生产构建）；`cargo test` 通过（35 项通过、1 项手工性能测试默认忽略且已单独执行通过）；`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`git diff --check` 通过。

## 5. 变更记录

| 日期 | 变更 | 结果 |
|---|---|---|
| 2026-08-17 | 新增 Search v2 规划与执行记录 | S0 完成，S1 开始 |
| 2026-08-17 | 完成 FTS5、条件解析、相关度排序与本地 provider | S1/S2 完成，S3 进行中 |
