// Agent sessions (Agent 会话): read-only discovery of local AI-agent CLI
// conversation transcripts plus trash-based deletion.
//
// Safety model (mirrors the cleaner family):
//   1. Discovery never writes — it walks each agent's transcript directory and
//      parses only the head/tail window of every file, so huge sessions stay
//      cheap to list.
//   2. Deletion always routes through `file_ops::trash_one` (OS trash, never
//     permanent) and only accepts paths that appear in a fresh discovery,
//     live under a registered agent root, and match the agent's naming rule.
//   3. Per-agent sidecar data (Claude sub-agent dirs, file history, session
//      env, todos; Codex `.jsonl.zst` twins; Gemini legacy twins) is trashed
//      together with the transcript so a "deleted" session cannot resurrect.
//   4. Whitelist is honoured fail-closed, exactly like the cleaner family.
//
// Adding a new agent means one entry in `PROVIDERS` — a root, a filename
// matcher and a parser. Everything else (cache, discovery, deletion,
// sidecars) is generic.

use crate::cleaner::{
    begin_cleaner_operation, load_whitelist, modified_secs, overlaps_whitelist, to_store_path,
    CleanOutcome, CleanRequestItem,
};
use crate::error::AppError;
use crate::file_ops::{self, OpFailure};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use walkdir::WalkDir;

// ── Data model (mirrored in src/types/sessions.ts) ────────────────────────────

/// One conversation session from one agent CLI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    /// Session id from the transcript (fallback: file stem).
    pub id: String,
    /// Provider id: `claude` | `codex` | `gemini`.
    pub agent: String,
    /// Working directory the session ran in ("" when unknown).
    pub project_dir: String,
    /// Basename of `project_dir` ("" when unknown).
    pub project_name: String,
    /// Display title: custom title → generated title → first user message →
    /// project name.
    pub title: String,
    /// Preview of the most recent message (≤ 160 chars).
    pub summary: String,
    /// Preview of the first real user message (≤ 160 chars).
    pub first_message: String,
    /// Epoch seconds, from the transcript when available, else file mtime.
    pub created_at: Option<i64>,
    /// Epoch seconds of the last transcript record.
    pub last_active_at: Option<i64>,
    pub size_bytes: u64,
    /// Canonical `/`-separated transcript path (store path convention).
    pub source_path: String,
    /// Shell command that resumes this session (run inside `project_dir`).
    pub resume_command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub session_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionsReport {
    pub agents: Vec<AgentInfo>,
    /// Newest first by last activity.
    pub sessions: Vec<AgentSession>,
}

// ── Tunables ──────────────────────────────────────────────────────────────────

/// Head lines inspected per transcript (titles, session_meta, first message).
const HEAD_LINES: usize = 12;
/// Tail lines inspected per transcript (last activity, last message preview).
const TAIL_LINES: usize = 40;
/// Tail reads seek back at most this many bytes instead of whole-file reads.
const TAIL_WINDOW_BYTES: u64 = 256 * 1024;
/// Cap per parsed line so a pathological multi-MB record cannot blow memory.
const MAX_LINE_BYTES: usize = 512 * 1024;
/// Whole-file cap for Gemini's single-JSON session format.
const GEMINI_READ_LIMIT: u64 = 8 * 1024 * 1024;
/// Preview/title truncation limits (chars, not bytes).
const PREVIEW_CHARS: usize = 160;
const TITLE_CHARS: usize = 120;

// ── Provider registry ─────────────────────────────────────────────────────────

struct Provider {
    id: &'static str,
    /// `$HOME`-relative roots scanned recursively.
    roots: &'static [&'static str],
    /// Filename rule deciding whether a walked file is a transcript.
    matches: fn(&Path) -> bool,
    /// Metadata extractor for one transcript file.
    parse: fn(&Ctx, &Path, u64) -> Option<AgentSession>,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: "claude",
        roots: &[".claude/projects"],
        matches: is_claude_transcript,
        parse: parse_claude,
    },
    Provider {
        id: "codex",
        roots: &[".codex/sessions", ".codex/archived_sessions"],
        matches: is_codex_rollout,
        parse: parse_codex,
    },
    Provider {
        id: "gemini",
        roots: &[".gemini/tmp"],
        matches: is_gemini_session,
        parse: parse_gemini,
    },
];

/// Parse context shared across one discovery pass.
struct Ctx {
    /// Codex session id → thread title (`session_index.jsonl`).
    codex_titles: HashMap<String, String>,
}

// ── Parse cache ───────────────────────────────────────────────────────────────

struct CacheEntry {
    modified: i64,
    len: u64,
    session: AgentSession,
}

/// Unchanged files reuse the previous parse result between scans.
static PARSE_CACHE: Mutex<Option<HashMap<PathBuf, CacheEntry>>> = Mutex::new(None);

// ── Discovery ─────────────────────────────────────────────────────────────────

pub fn discover_sessions() -> Result<AgentSessionsReport, AppError> {
    let home =
        home_dir().ok_or_else(|| AppError::new("SESSIONS_UNAVAILABLE", "无法定位用户主目录。"))?;
    discover_in_home(&home)
}

/// Home-injected core, shared with tests.
fn discover_in_home(home: &Path) -> Result<AgentSessionsReport, AppError> {
    let ctx = Ctx {
        codex_titles: load_codex_titles(home),
    };
    let mut agents = Vec::new();
    let mut sessions: Vec<AgentSession> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for provider in PROVIDERS {
        let before = sessions.len();
        for relative in provider.roots {
            let root = home.join(relative);
            if !root.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&root)
                .max_depth(6)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if !(provider.matches)(path) {
                    continue;
                }
                if let Some(session) = parse_with_cache(&ctx, provider, path) {
                    seen.insert(path.to_path_buf());
                    sessions.push(session);
                }
            }
        }
        agents.push(AgentInfo {
            id: provider.id.to_owned(),
            session_count: (sessions.len() - before) as u64,
        });
    }

    trim_cache(&seen);
    sessions.sort_by(|a, b| {
        let a_key = a.last_active_at.or(a.created_at);
        let b_key = b.last_active_at.or(b.created_at);
        b_key.cmp(&a_key).then_with(|| a.id.cmp(&b.id))
    });
    Ok(AgentSessionsReport { agents, sessions })
}

fn parse_with_cache(ctx: &Ctx, provider: &Provider, path: &Path) -> Option<AgentSession> {
    let meta = fs::metadata(path).ok()?;
    let modified = modified_secs(&meta).unwrap_or(0);
    let len = meta.len();
    let key = path.to_path_buf();

    let mut guard = match PARSE_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(entry) = cache.get(&key) {
        if entry.modified == modified && entry.len == len {
            return Some(entry.session.clone());
        }
    }
    let session = (provider.parse)(ctx, path, len)?;
    cache.insert(
        key,
        CacheEntry {
            modified,
            len,
            session: session.clone(),
        },
    );
    Some(session)
}

/// Drop cache rows for transcripts that no longer exist.
fn trim_cache(seen: &HashSet<PathBuf>) {
    let mut guard = match PARSE_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(cache) = guard.as_mut() {
        cache.retain(|path, _| seen.contains(path));
    }
}

// ── Deletion ──────────────────────────────────────────────────────────────────

/// Trash confirmed sessions. Re-discovers first and verifies every request
/// against the fresh result (path + size), the cleaner-family whitelist and
/// the generic write-path protection before anything is touched.
pub fn trash_sessions(
    db_path: &Path,
    items: &[CleanRequestItem],
) -> Result<CleanOutcome, AppError> {
    let home =
        home_dir().ok_or_else(|| AppError::new("SESSIONS_UNAVAILABLE", "无法定位用户主目录。"))?;
    trash_sessions_in(db_path, &home, items, &|native| file_ops::trash_one(native))
}

/// Deleter-injected core, shared with tests.
fn trash_sessions_in(
    db_path: &Path,
    home: &Path,
    items: &[CleanRequestItem],
    deleter: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<CleanOutcome, AppError> {
    // Fail closed: if the whitelist is unreadable we refuse to delete, same
    // as the cleaner family.
    let whitelist = load_whitelist(db_path)?;
    let report = discover_in_home(home)?;
    let fresh: HashMap<&str, &AgentSession> = report
        .sessions
        .iter()
        .map(|session| (session.source_path.as_str(), session))
        .collect();

    let operation_id = begin_cleaner_operation(
        db_path,
        format!("会话清理（{} 项，移至废纸篓）", items.len()),
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for item in items {
        let outcome = match fresh.get(item.path.as_str()) {
            Some(session) => verify_session(home, session, item, &whitelist),
            None => Err(AppError::new(
                "PATH_NOT_IN_SCOPE",
                format!("路径不在当前会话发现结果中，请重新扫描：{}", item.path),
            )),
        };
        match outcome {
            Ok((native, sidecars)) => match deleter(&native) {
                Ok(()) => {
                    // Sidecars are best effort: the transcript is what makes
                    // the session, the rest is auxiliary data already in the
                    // trash with it.
                    for sidecar in &sidecars {
                        let _ = deleter(sidecar);
                    }
                    succeeded.push(item.path.clone());
                }
                Err(reason) => failed.push(OpFailure {
                    path: item.path.clone(),
                    reason,
                }),
            },
            Err(error) => failed.push(OpFailure {
                path: item.path.clone(),
                reason: error.message,
            }),
        }
    }

    file_ops::finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(CleanOutcome {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

/// Deletion-boundary checks for one session; returns the native transcript
/// path plus its existing sidecar paths.
fn verify_session(
    home: &Path,
    session: &AgentSession,
    item: &CleanRequestItem,
    whitelist: &[String],
) -> Result<(PathBuf, Vec<PathBuf>), AppError> {
    if item.size_bytes != session.size_bytes {
        return Err(AppError::new(
            "FILE_CHANGED",
            format!("会话大小与重新扫描结果不一致，请刷新后重试：{}", item.path),
        ));
    }
    let native = file_ops::validate_write_path(&item.path)?;
    if overlaps_whitelist(&item.path, whitelist, cfg!(windows)) {
        return Err(AppError::new(
            "WHITELISTED_PATH",
            format!("路径受白名单保护：{}", item.path),
        ));
    }
    let sidecars = sidecar_paths(&session.agent, &native, home);
    Ok((native, sidecars))
}

/// Existing per-agent data that belongs to the transcript being deleted.
fn sidecar_paths(agent: &str, source: &Path, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(stem) = source.file_stem().and_then(OsStr::to_str) else {
        return out;
    };
    let push_if_exists = |out: &mut Vec<PathBuf>, path: PathBuf| {
        if path.exists() {
            out.push(path);
        }
    };
    match agent {
        "claude" => {
            // Sub-agent transcripts and persisted tool outputs live in a
            // sibling `<session-id>/` directory.
            if let Some(parent) = source.parent() {
                push_if_exists(&mut out, parent.join(stem));
            }
            push_if_exists(&mut out, home.join(".claude/file-history").join(stem));
            push_if_exists(&mut out, home.join(".claude/session-env").join(stem));
            // Todos are keyed by session id with assorted suffixes.
            if let Ok(entries) = fs::read_dir(home.join(".claude/todos")) {
                for entry in entries.filter_map(Result::ok) {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with(stem) && name.ends_with(".json") {
                        out.push(entry.path());
                    }
                }
            }
        }
        "codex" => {
            // Deleting only the plain twin lets the session resurrect from the
            // compressed one.
            let mut zst = source.as_os_str().to_owned();
            zst.push(".zst");
            push_if_exists(&mut out, PathBuf::from(zst));
        }
        "gemini" => {
            // Legacy twin of the other extension (session-*.json/.jsonl).
            let twin = match source.extension().and_then(OsStr::to_str) {
                Some("jsonl") => source.with_extension("json"),
                Some("json") => source.with_extension("jsonl"),
                _ => return out,
            };
            push_if_exists(&mut out, twin);
        }
        _ => {}
    }
    out
}

// ── Filename rules ────────────────────────────────────────────────────────────

fn file_name_of(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

fn is_claude_transcript(path: &Path) -> bool {
    let Some(name) = file_name_of(path) else {
        return false;
    };
    name.ends_with(".jsonl")
        && !name.starts_with("agent-")
        && name != "journal.jsonl"
        && !name.starts_with('.')
}

fn is_codex_rollout(path: &Path) -> bool {
    let Some(name) = file_name_of(path) else {
        return false;
    };
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn is_gemini_session(path: &Path) -> bool {
    let Some(name) = file_name_of(path) else {
        return false;
    };
    if name.starts_with("session-") && name.ends_with(".jsonl") {
        return true;
    }
    if name.starts_with("session-") && name.ends_with(".json") {
        // When the newer .jsonl replay twin exists, only it counts.
        return !path.with_extension("jsonl").exists();
    }
    false
}

// ── Claude Code ───────────────────────────────────────────────────────────────

const CLAUDE_INJECTED_PREFIXES: &[&str] = &[
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<caveat>",
    "Caveat:",
    "<task-notification>",
    "<system-reminder>",
];

/// `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`
fn parse_claude(_ctx: &Ctx, path: &Path, size_bytes: u64) -> Option<AgentSession> {
    let head = read_head_lines(path, HEAD_LINES);
    let tail = read_tail_lines(path, TAIL_LINES);
    if head.is_empty() && tail.is_empty() {
        return None;
    }

    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut summary_line: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut first_message: Option<String> = None;

    for line in &head {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if cwd.is_none() {
            cwd = str_field(&value, "cwd").map(str::to_owned);
        }
        if created_at.is_none() {
            created_at = str_field(&value, "timestamp").and_then(parse_rfc3339_secs);
        }
        match str_field(&value, "type") {
            Some("custom-title") => {
                custom_title = str_field(&value, "customTitle").map(clean_text);
            }
            Some("ai-title") => ai_title = str_field(&value, "aiTitle").map(clean_text),
            Some("summary") => summary_line = str_field(&value, "summary").map(clean_text),
            Some("user")
                if first_message.is_none()
                    && value.get("isSidechain").and_then(Value::as_bool) != Some(true) =>
            {
                first_message = claude_message_texts(&value)
                    .into_iter()
                    .find(|text| !is_claude_injected(text));
            }
            _ => {}
        }
    }

    let mut last_active_at: Option<i64> = None;
    let mut last_message: Option<String> = None;
    for line in tail.iter().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if last_active_at.is_none() {
            last_active_at = str_field(&value, "timestamp").and_then(parse_rfc3339_secs);
        }
        if last_message.is_none() {
            if let Some(role) = value.pointer("/message/role").and_then(Value::as_str) {
                if role == "assistant" || role == "user" {
                    last_message = claude_message_texts(&value)
                        .into_iter()
                        .rev()
                        .find(|text| !is_claude_injected(text));
                }
            }
        }
    }

    let project_dir = cwd.unwrap_or_default();
    // With no in-file cwd the encoded directory name ("-Users-x-repo") is the
    // only identity left — it is lossy but displayable.
    let project_name = if project_dir.is_empty() {
        path.parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .to_owned()
    } else {
        basename_string(&project_dir)
    };

    let id = file_stem_string(path);
    let title = custom_title
        .or(ai_title)
        .or(first_message.clone())
        .or(summary_line)
        .unwrap_or_else(|| fallback_title(&project_name, &id));
    let summary = last_message.unwrap_or_default();

    Some(build_session(
        SessionDraft {
            agent: "claude",
            resume_command: format!("claude --resume {id}"),
            id,
            project_dir,
            project_name,
            title,
            first_message: first_message.unwrap_or_default(),
            summary,
            created_at,
            last_active_at,
        },
        path,
        size_bytes,
    ))
}

fn claude_message_texts(line: &Value) -> Vec<String> {
    line.pointer("/message/content")
        .map(message_texts)
        .unwrap_or_default()
}

fn is_claude_injected(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.is_empty()
        || CLAUDE_INJECTED_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
}

// ── Codex CLI ─────────────────────────────────────────────────────────────────

const CODEX_INJECTED_PREFIXES: &[&str] = &[
    "<environment_context>",
    "<user_instructions>",
    "# AGENTS.md",
];

/// `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
fn parse_codex(ctx: &Ctx, path: &Path, size_bytes: u64) -> Option<AgentSession> {
    let head = read_head_lines(path, HEAD_LINES);
    let tail = read_tail_lines(path, TAIL_LINES);
    if head.is_empty() {
        return None;
    }

    // Line 1 should be the session_meta record; search defensively anyway.
    let meta_payload = head
        .iter()
        .find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            (str_field(&value, "type") == Some("session_meta"))
                .then(|| value.get("payload").cloned())
                .flatten()
        })
        .filter(|payload| payload.is_object())?;

    let id = str_field(&meta_payload, "id")
        .map(str::to_owned)
        .unwrap_or_else(|| file_stem_string(path));
    let project_dir = str_field(&meta_payload, "cwd").unwrap_or("").to_owned();
    let created_at = str_field(&meta_payload, "timestamp")
        .and_then(parse_rfc3339_secs)
        .or_else(|| {
            head.iter().find_map(|line| {
                serde_json::from_str::<Value>(line)
                    .ok()
                    .and_then(|value| str_field(&value, "timestamp").and_then(parse_rfc3339_secs))
            })
        });

    let mut first_message: Option<String> = None;
    for line in &head {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if str_field(&value, "type") != Some("response_item") {
            continue;
        }
        let payload = &value["payload"];
        if str_field(payload, "type") != Some("message")
            || str_field(payload, "role") != Some("user")
        {
            continue;
        }
        let texts = match payload.get("content") {
            Some(content) => message_texts(content),
            None => Vec::new(),
        };
        if let Some(text) = texts
            .into_iter()
            .filter_map(|text| normalize_codex_user_text(&text))
            .next()
        {
            first_message = Some(text);
            break;
        }
    }

    let mut last_active_at: Option<i64> = None;
    let mut last_message: Option<String> = None;
    for line in tail.iter().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if last_active_at.is_none() {
            last_active_at = str_field(&value, "timestamp").and_then(parse_rfc3339_secs);
        }
        if last_message.is_none() {
            if str_field(&value, "type") == Some("event_msg")
                && str_field(&value["payload"], "type") == Some("agent_message")
            {
                last_message = value
                    .pointer("/payload/message")
                    .and_then(Value::as_str)
                    .map(clean_text);
            } else if str_field(&value, "type") == Some("response_item")
                && str_field(&value["payload"], "role") == Some("assistant")
            {
                let mut texts = message_texts(&value["payload"]["content"]);
                last_message = texts.pop();
            }
        }
    }

    let project_name = basename_string(&project_dir);
    let title = ctx
        .codex_titles
        .get(&id)
        .cloned()
        .or(first_message.clone())
        .unwrap_or_else(|| fallback_title(&project_name, &id));
    let summary = last_message.unwrap_or_default();

    Some(build_session(
        SessionDraft {
            agent: "codex",
            resume_command: format!("codex resume {id}"),
            id,
            project_dir,
            project_name,
            title,
            first_message: first_message.unwrap_or_default(),
            summary,
            created_at,
            last_active_at,
        },
        path,
        size_bytes,
    ))
}

/// Skip injected instruction blocks; surface the real request from VS
/// Code-originated sessions ("## My request for Codex: …").
fn normalize_codex_user_text(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.is_empty()
        || CODEX_INJECTED_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
    {
        return None;
    }
    if let Some(index) = trimmed.rfind("## My request for Codex:") {
        let request = trimmed[index + "## My request for Codex:".len()..].trim();
        if request.is_empty() {
            return None;
        }
        return Some(request.to_owned());
    }
    Some(trimmed.to_owned())
}

/// Codex thread titles from `~/.codex/session_index.jsonl` (`{id, thread_name}`).
fn load_codex_titles(home: &Path) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let Ok(content) = fs::read_to_string(home.join(".codex/session_index.jsonl")) else {
        return titles;
    };
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let (Some(id), Some(title)) = (str_field(&value, "id"), str_field(&value, "thread_name"))
        {
            if !title.trim().is_empty() {
                titles.insert(id.to_owned(), title.to_owned());
            }
        }
    }
    titles
}

// ── Gemini CLI ────────────────────────────────────────────────────────────────

const GEMINI_INJECTED_PREFIXES: &[&str] = &["<session_context>"];

/// `~/.gemini/tmp/<project-key>/chats/session-*.json(l)`; the key dir holds a
/// `.project_root` marker with the real working directory.
fn parse_gemini(_ctx: &Ctx, path: &Path, size_bytes: u64) -> Option<AgentSession> {
    let project_dir = gemini_project_dir(path).unwrap_or_default();
    let is_jsonl = path.extension() == Some(OsStr::new("jsonl"));

    let mut doc = GeminiDoc::default();
    if is_jsonl {
        let mut lines = read_head_lines(path, HEAD_LINES);
        lines.extend(read_tail_lines(path, TAIL_LINES));
        for line in &lines {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                collect_gemini(&value, &mut doc);
            }
        }
    } else if let Some(value) = read_json_capped(path) {
        collect_gemini(&value, &mut doc);
    }

    // Fallbacks for unreadable/oversized transcripts: still show the session
    // with file-derived metadata.
    let id = doc.session_id.unwrap_or_else(|| file_stem_string(path));
    let created_at = doc.start.or_else(|| file_modified_secs(path));
    let last_active_at = doc.updated.or(created_at);
    let project_name = basename_string(&project_dir);

    let first_message = doc
        .messages
        .iter()
        .filter(|message| str_field(message, "type") == Some("user"))
        .flat_map(gemini_message_texts)
        .find(|text| !is_gemini_ignored(text));
    let last_message = doc
        .messages
        .iter()
        .rev()
        .filter_map(|message| {
            let mut texts = gemini_message_texts(message);
            texts.pop()
        })
        .find(|text| !text.trim().is_empty());

    let title = first_message
        .clone()
        .unwrap_or_else(|| fallback_title(&project_name, &id));
    let summary = last_message.unwrap_or_default();

    Some(build_session(
        SessionDraft {
            agent: "gemini",
            resume_command: format!("gemini --resume {id}"),
            id,
            project_dir,
            project_name,
            title,
            first_message: first_message.unwrap_or_default(),
            summary,
            created_at,
            last_active_at,
        },
        path,
        size_bytes,
    ))
}

#[derive(Default)]
struct GeminiDoc {
    session_id: Option<String>,
    start: Option<i64>,
    updated: Option<i64>,
    messages: Vec<Value>,
}

/// Collect the fields we need from either the single-JSON document or a
/// replay-format line (fields may sit under a `$set` op).
fn collect_gemini(value: &Value, doc: &mut GeminiDoc) {
    if let Some(id) = str_field(value, "sessionId") {
        doc.session_id = Some(id.to_owned());
    }
    if let Some(ts) = value.get("startTime").and_then(parse_ts_value) {
        doc.start = Some(ts);
    }
    if let Some(ts) = value.get("lastUpdated").and_then(parse_ts_value) {
        doc.updated = Some(ts);
    }
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        doc.messages.extend(messages.iter().cloned());
    }
    if let Some(set) = value.get("$set") {
        collect_gemini(set, doc);
    }
}

fn gemini_message_texts(message: &Value) -> Vec<String> {
    match message.get("content") {
        Some(content) => message_texts(content),
        None => Vec::new(),
    }
}

fn gemini_project_dir(path: &Path) -> Option<String> {
    let chats = path.parent()?;
    let key_dir = chats.parent()?;
    let marker = fs::read_to_string(key_dir.join(".project_root")).ok()?;
    let trimmed = marker.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn is_gemini_ignored(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('?')
        || GEMINI_INJECTED_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
}

// ── Shared parsing helpers ────────────────────────────────────────────────────

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Text content of one message: a plain string, or an array whose text-ish
/// blocks carry a `text` field (`text` / `input_text` / `output_text` blocks;
/// tool calls, thinking blocks and tool results have no `text` field and are
/// skipped).
fn message_texts(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                other => other.get("text").and_then(Value::as_str).map(clean_text),
            })
            .map(|text| clean_text(&text))
            .filter(|text| !text.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn clean_text(text: &str) -> String {
    text.trim().to_owned()
}

/// Everything one parse knows about a session, before the shared finishing
/// pass (truncation, store-path mapping) that turns it into an `AgentSession`.
struct SessionDraft {
    agent: &'static str,
    id: String,
    project_dir: String,
    project_name: String,
    title: String,
    first_message: String,
    summary: String,
    created_at: Option<i64>,
    last_active_at: Option<i64>,
    resume_command: String,
}

fn build_session(draft: SessionDraft, path: &Path, size_bytes: u64) -> AgentSession {
    AgentSession {
        id: draft.id,
        agent: draft.agent.to_owned(),
        project_dir: draft.project_dir,
        project_name: draft.project_name,
        title: truncate_chars(&draft.title, TITLE_CHARS),
        summary: truncate_chars(&draft.summary, PREVIEW_CHARS),
        first_message: truncate_chars(&draft.first_message, PREVIEW_CHARS),
        created_at: draft.created_at,
        last_active_at: draft.last_active_at,
        size_bytes,
        source_path: to_store_path(path),
        resume_command: draft.resume_command,
    }
}

/// Basename of a directory path ("" when empty/relative-degenerate).
fn basename_string(dir: &str) -> String {
    Path::new(dir)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_owned()
}

fn fallback_title(project_name: &str, id: &str) -> String {
    if project_name.is_empty() {
        id.to_owned()
    } else {
        project_name.to_owned()
    }
}

fn file_stem_string(path: &Path) -> String {
    path.file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_owned()
}

fn file_modified_secs(path: &Path) -> Option<i64> {
    modified_secs(&fs::metadata(path).ok()?)
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(max).collect();
    format!("{truncated}…")
}

// ── Windowed reads ────────────────────────────────────────────────────────────

fn read_head_lines(path: &Path, max: usize) -> Vec<String> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    // BufReader keeps unconsumed bytes across calls — a raw `File::read`
    // would throw away everything past the first line of a small file.
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut buf = Vec::new();
    for _ in 0..max {
        buf.clear();
        // `take` caps the bytes of any single pathological line.
        match reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64)
            .read_until(b'\n', &mut buf)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        lines.push(String::from_utf8_lossy(&buf).into_owned());
    }
    lines
}

fn read_tail_lines(path: &Path, max: usize) -> Vec<String> {
    let Ok(meta) = fs::metadata(path) else {
        return Vec::new();
    };
    let len = meta.len();
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let window = len.min(TAIL_WINDOW_BYTES);
    if window < len && file.seek(SeekFrom::End(-(window as i64))).is_err() {
        return Vec::new();
    }
    let mut data = Vec::with_capacity(window as usize);
    if file.read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&data);
    let mut lines: Vec<&str> = text.lines().collect();
    if window < len && !lines.is_empty() {
        // The byte window may start mid-line; drop that partial line.
        lines.remove(0);
    }
    let start = lines.len().saturating_sub(max);
    lines.into_iter().skip(start).map(str::to_owned).collect()
}

fn read_json_capped(path: &Path) -> Option<Value> {
    let len = fs::metadata(path).ok()?.len();
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; len.min(GEMINI_READ_LIMIT) as usize];
    file.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

// ── Timestamps ────────────────────────────────────────────────────────────────

/// RFC 3339 / ISO 8601 timestamp → epoch seconds, without pulling in chrono.
/// Accepts `2026-11-06T16:16:43(.…)?(Z|±HH:MM)?` (missing offset = UTC).
fn parse_rfc3339_secs(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let bytes = raw.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let month: i64 = raw.get(5..7)?.parse().ok()?;
    let day: i64 = raw.get(8..10)?.parse().ok()?;
    let separator = bytes[10];
    if separator != b'T' && separator != b't' && separator != b' ' {
        return None;
    }
    let hour: i64 = raw.get(11..13)?.parse().ok()?;
    if bytes[13] != b':' {
        return None;
    }
    let minute: i64 = raw.get(14..16)?.parse().ok()?;
    if bytes[16] != b':' {
        return None;
    }
    let second: i64 = raw.get(17..19)?.parse().ok()?;

    let mut rest = &raw[19..];
    if rest.starts_with('.') || rest.starts_with(',') {
        let end = rest[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|index| index + 1)
            .unwrap_or(rest.len());
        rest = &rest[end..];
    }
    let mut offset_secs: i64 = 0;
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let digits = &rest[1..];
        let mut parts = digits.split(':');
        let hh: i64 = parts.next().unwrap_or("").parse().ok()?;
        let mm: i64 = parts.next().unwrap_or("0").trim().parse().ok()?;
        let factor: i64 = if sign == '-' { -1 } else { 1 };
        offset_secs = factor * (hh * 3_600 + mm * 60);
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset_secs)
}

/// Epoch seconds or milliseconds (Gemini sometimes stores plain numbers).
fn parse_ts_value(value: &Value) -> Option<i64> {
    if let Some(raw) = value.as_str() {
        return parse_rfc3339_secs(raw);
    }
    if let Some(number) = value.as_f64() {
        let secs = if number.abs() > 1.0e12 {
            number / 1000.0
        } else {
            number
        };
        return Some(secs as i64);
    }
    None
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = if shifted_year >= 0 {
        shifted_year
    } else {
        shifted_year - 399
    } / 400;
    let year_of_era = shifted_year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target/luma-sessions-tests");
        let dir = base.join(format!("{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx_with_titles(titles: &[(&str, &str)]) -> Ctx {
        Ctx {
            codex_titles: titles
                .iter()
                .map(|(id, title)| (id.to_string(), title.to_string()))
                .collect(),
        }
    }

    // ── Timestamps ──

    #[test]
    fn rfc3339_utc() {
        assert_eq!(
            parse_rfc3339_secs("2026-11-06T16:16:43Z"),
            Some(1_793_981_803)
        );
        assert_eq!(
            parse_rfc3339_secs("2026-11-06T16:16:43.123Z"),
            Some(1_793_981_803)
        );
    }

    #[test]
    fn rfc3339_offset() {
        assert_eq!(
            parse_rfc3339_secs("2026-11-06T16:16:43+08:00"),
            Some(1_793_981_803 - 28_800)
        );
        assert_eq!(
            parse_rfc3339_secs("2026-11-06T16:16:43-01:30"),
            Some(1_793_981_803 + 5_400)
        );
    }

    #[test]
    fn rfc3339_rejects_garbage() {
        assert_eq!(parse_rfc3339_secs(""), None);
        assert_eq!(parse_rfc3339_secs("not-a-time"), None);
        assert_eq!(parse_rfc3339_secs("2026-11-06"), None);
    }

    #[test]
    fn ts_value_accepts_epoch_millis() {
        assert_eq!(
            parse_ts_value(&Value::from(1_793_981_803_000_f64)),
            Some(1_793_981_803)
        );
        assert_eq!(
            parse_ts_value(&Value::from(1_793_981_803)),
            Some(1_793_981_803)
        );
    }

    // ── Text helpers ──

    #[test]
    fn truncate_chars_is_char_safe() {
        assert_eq!(truncate_chars("你好世界", 2), "你好…");
        assert_eq!(truncate_chars("short", 10), "short");
    }

    #[test]
    fn message_texts_skips_tool_blocks() {
        let value = serde_json::json!([
            {"type": "text", "text": " hello "},
            {"type": "thinking", "thinking": "hidden"},
            {"type": "tool_use", "id": "t1"},
            "plain"
        ]);
        assert_eq!(message_texts(&value), vec!["hello", "plain"]);
    }

    // ── Filename rules ──

    #[test]
    fn claude_rules_skip_subagent_files() {
        assert!(is_claude_transcript(Path::new("/p/-Users-x/8f2b.jsonl")));
        assert!(!is_claude_transcript(Path::new(
            "/p/-Users-x/8f2b/subagents/agent-abc.jsonl"
        )));
        assert!(!is_claude_transcript(Path::new(
            "/p/-Users-x/journal.jsonl"
        )));
    }

    #[test]
    fn codex_rules_accept_rollout_only() {
        assert!(is_codex_rollout(Path::new(
            "/s/2026/11/06/rollout-2026-11-06T16-16-43-01a1.jsonl"
        )));
        assert!(!is_codex_rollout(Path::new(
            "/s/2026/11/06/rollout-2026.jsonl.zst"
        )));
        assert!(!is_codex_rollout(Path::new("/s/2026/11/06/other.jsonl")));
    }

    #[test]
    fn gemini_rules_prefer_jsonl_twin() {
        let root = temp_root("gemini-rules");
        let chats = root.join("tmp/k/chats");
        fs::create_dir_all(&chats).unwrap();
        let json = chats.join("session-1.json");
        let jsonl = chats.join("session-1.jsonl");
        fs::write(&json, "{}").unwrap();
        assert!(is_gemini_session(&json));
        fs::write(&jsonl, "{}").unwrap();
        assert!(!is_gemini_session(&json));
        assert!(is_gemini_session(&jsonl));
    }

    // ── Claude parsing ──

    fn claude_fixture() -> String {
        [
            r#"{"type":"mode","mode":"bypassPermissions"}"#,
            r#"{"type":"file-history-snapshot","messageId":"m0"}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"2026-11-05T09:00:00.500Z","isSidechain":false,"cwd":"/Users/x/repo","sessionId":"sid-1","message":{"role":"user","content":"<command-name>/init</command-name>\n<command-message>ignored</command-message>"}}"#,
            r#"{"type":"user","uuid":"u2","timestamp":"2026-11-05T09:00:01Z","isSidechain":false,"cwd":"/Users/x/repo","sessionId":"sid-1","message":{"role":"user","content":"帮我修一下登录 bug"}}"#,
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-05T09:00:05Z","cwd":"/Users/x/repo","message":{"role":"assistant","model":"x","content":[{"type":"text","text":"好的，我来看登录模块。"}]}}"#,
            r#"{"type":"ai-title","aiTitle":"修复登录 bug","sessionId":"sid-1"}"#,
            r#"{"type":"user","uuid":"u3","timestamp":"2026-11-05T10:30:00Z","isSidechain":false,"message":{"role":"user","content":"请顺手加个测试"}}"#,
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-11-05T10:31:12Z","message":{"role":"assistant","content":[{"type":"text","text":"测试已补上，全部通过。"}]}}"#,
        ]
        .join("\n")
    }

    #[test]
    fn parse_claude_extracts_metadata() {
        let root = temp_root("claude-parse");
        let project_dir = root.join("projects/-Users-x-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("sid-1.jsonl");
        fs::write(&file, claude_fixture()).unwrap();

        let session = parse_claude(&ctx_with_titles(&[]), &file, 1234).expect("parses");
        assert_eq!(session.id, "sid-1");
        assert_eq!(session.agent, "claude");
        // The injected /init command block is skipped for the first message.
        assert_eq!(session.first_message, "帮我修一下登录 bug");
        // Title chain: ai-title wins over the first user message.
        assert_eq!(session.title, "修复登录 bug");
        assert_eq!(session.summary, "测试已补上，全部通过。");
        assert_eq!(session.project_dir, "/Users/x/repo");
        assert_eq!(session.project_name, "repo");
        assert_eq!(session.created_at, Some(1_793_869_200));
        assert_eq!(session.last_active_at, Some(1_793_874_672));
        assert_eq!(session.size_bytes, 1234);
        assert_eq!(session.resume_command, "claude --resume sid-1");
    }

    #[test]
    fn parse_claude_falls_back_without_titles() {
        let root = temp_root("claude-fallback");
        let project_dir = root.join("projects/-Users-x-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("sid-2.jsonl");
        fs::write(
            &file,
            r#"{"type":"user","timestamp":"2026-11-05T09:00:00Z","cwd":"/Users/x/repo","message":{"role":"user","content":"直接的问题"}}"#,
        )
        .unwrap();

        let session = parse_claude(&ctx_with_titles(&[]), &file, 10).expect("parses");
        assert_eq!(session.title, "直接的问题");
        // Single-record transcript: the last message is the first message.
        assert_eq!(session.summary, "直接的问题");
    }

    // ── Codex parsing ──

    fn codex_fixture() -> String {
        [
            r#"{"timestamp":"2026-11-06T16:16:43.123Z","ordinal":0,"type":"session_meta","payload":{"id":"codex-1","timestamp":"2026-11-06T16:16:43Z","cwd":"/Users/x/app","originator":"codex-tui","cli_version":"0.167.0"}}"#,
            r###"{"timestamp":"2026-11-06T16:16:44Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md\n<instructions/>"}]}}"###,
            r#"{"timestamp":"2026-11-06T16:16:45Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>env</environment_context>"}]}}"#,
            r###"{"timestamp":"2026-11-06T16:16:46Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"## My request for Codex:\n重构会话解析器"}]}}"###,
            r#"{"timestamp":"2026-11-06T16:20:00Z","type":"event_msg","payload":{"type":"agent_message","message":"解析器已重构完成。"}}"#,
        ]
        .join("\n")
    }

    #[test]
    fn parse_codex_extracts_metadata() {
        let root = temp_root("codex-parse");
        let day_dir = root.join("sessions/2026/11/06");
        fs::create_dir_all(&day_dir).unwrap();
        let file = day_dir.join("rollout-2026-11-06T16-16-43-codex-1.jsonl");
        fs::write(&file, codex_fixture()).unwrap();

        let ctx = ctx_with_titles(&[("codex-1", "索引文件线程标题")]);
        let session = parse_codex(&ctx, &file, 2048).expect("parses");
        assert_eq!(session.id, "codex-1");
        assert_eq!(session.title, "索引文件线程标题");
        assert_eq!(session.project_dir, "/Users/x/app");
        assert_eq!(session.project_name, "app");
        assert_eq!(session.created_at, Some(1_793_981_803));
        assert_eq!(session.last_active_at, Some(1_793_982_000));
        assert_eq!(session.first_message, "重构会话解析器");
        assert_eq!(session.summary, "解析器已重构完成。");
        assert_eq!(session.resume_command, "codex resume codex-1");
    }

    #[test]
    fn parse_codex_title_falls_back_to_first_message() {
        let root = temp_root("codex-fallback");
        let day_dir = root.join("sessions/2026/11/06");
        fs::create_dir_all(&day_dir).unwrap();
        let file = day_dir.join("rollout-x.jsonl");
        fs::write(&file, codex_fixture()).unwrap();

        let session = parse_codex(&ctx_with_titles(&[]), &file, 1).expect("parses");
        assert_eq!(session.title, "重构会话解析器");
    }

    // ── Gemini parsing ──

    #[test]
    fn parse_gemini_json_extracts_metadata() {
        let root = temp_root("gemini-parse");
        let chats = root.join("tmp/my-project/chats");
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            root.join("tmp/my-project/.project_root"),
            "/Users/x/real-root\n",
        )
        .unwrap();
        let file = chats.join("session-1.json");
        fs::write(
            &file,
            r#"{"sessionId":"gem-1","projectHash":"abc","startTime":"2026-11-04T08:00:00Z","lastUpdated":"2026-11-04T09:30:00Z","kind":"main","messages":[{"id":"m1","type":"user","content":[{"text":"/help"}]},{"id":"m2","type":"user","content":[{"text":"看看构建日志"}]},{"id":"m3","type":"assistant","content":[{"text":"日志显示缺依赖。"}]}]}"#,
        )
        .unwrap();

        let session = parse_gemini(&ctx_with_titles(&[]), &file, 77).expect("parses");
        assert_eq!(session.id, "gem-1");
        assert_eq!(session.agent, "gemini");
        assert_eq!(session.project_dir, "/Users/x/real-root");
        assert_eq!(session.project_name, "real-root");
        // Slash commands are not treated as the title.
        assert_eq!(session.first_message, "看看构建日志");
        assert_eq!(session.title, "看看构建日志");
        assert_eq!(session.summary, "日志显示缺依赖。");
        assert_eq!(session.created_at, Some(1_793_779_200));
        assert_eq!(session.last_active_at, Some(1_793_784_600));
        assert_eq!(session.resume_command, "gemini --resume gem-1");
    }

    #[test]
    fn parse_gemini_jsonl_replay_format() {
        let root = temp_root("gemini-jsonl");
        let chats = root.join("tmp/k2/chats");
        fs::create_dir_all(&chats).unwrap();
        let file = chats.join("session-2.jsonl");
        fs::write(
            &file,
            [
                r#"{"$set":{"sessionId":"gem-2","startTime":"2026-11-04T08:00:00Z","lastUpdated":"2026-11-04T08:05:00Z"}}"#,
                r#"{"$rewindTo":3}"#,
                r#"{"messages":[{"type":"user","content":[{"text":"检查 CI"}]},{"type":"assistant","content":[{"text":"CI 已通过。"}]}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let session = parse_gemini(&ctx_with_titles(&[]), &file, 5).expect("parses");
        assert_eq!(session.id, "gem-2");
        assert_eq!(session.first_message, "检查 CI");
        assert_eq!(session.summary, "CI 已通过。");
    }

    // ── Sidecars ──

    #[test]
    fn claude_sidecars_cover_dir_history_env_todos() {
        let home = temp_root("claude-sidecars");
        let transcript = home.join(".claude/projects/p/sid-1.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::write(&transcript, "{}").unwrap();
        fs::create_dir_all(home.join(".claude/projects/p/sid-1/subagents")).unwrap();
        fs::create_dir_all(home.join(".claude/file-history/sid-1")).unwrap();
        fs::create_dir_all(home.join(".claude/session-env/sid-1")).unwrap();
        fs::create_dir_all(home.join(".claude/todos")).unwrap();
        fs::write(home.join(".claude/todos/sid-1-agent-1.json"), "{}").unwrap();
        fs::write(home.join(".claude/todos/other.json"), "{}").unwrap();

        let sidecars = sidecar_paths("claude", &transcript, &home);
        let names: Vec<String> = sidecars
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 4, "got {names:?}");
        assert!(names.iter().any(|n| n.ends_with("/p/sid-1")));
        assert!(names.iter().any(|n| n.contains("file-history/sid-1")));
        assert!(names.iter().any(|n| n.contains("session-env/sid-1")));
        assert!(names
            .iter()
            .any(|n| n.ends_with("todos/sid-1-agent-1.json")));
        assert!(!names.iter().any(|n| n.ends_with("todos/other.json")));
    }

    #[test]
    fn codex_sidecars_include_zst_twin() {
        let home = temp_root("codex-sidecars");
        let transcript = home.join("sessions/rollout-x.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::write(&transcript, "{}").unwrap();
        fs::write(home.join("sessions/rollout-x.jsonl.zst"), "z").unwrap();

        let sidecars = sidecar_paths("codex", &transcript, &home);
        assert_eq!(sidecars.len(), 1);
        assert!(sidecars[0]
            .to_string_lossy()
            .ends_with("rollout-x.jsonl.zst"));
    }

    #[test]
    fn gemini_sidecars_include_legacy_twin() {
        let home = temp_root("gemini-sidecars");
        let transcript = home.join("chats/session-1.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::write(&transcript, "{}").unwrap();
        fs::write(transcript.parent().unwrap().join("session-1.json"), "{}").unwrap();

        let sidecars = sidecar_paths("gemini", &transcript, &home);
        assert_eq!(sidecars.len(), 1);
        assert!(sidecars[0].to_string_lossy().ends_with("session-1.json"));
    }

    // ── Discovery ──

    #[test]
    fn discover_in_home_groups_by_agent_and_skips_subagents() {
        let home = temp_root("discover");
        // Claude: one real session + one sub-agent transcript that must be skipped.
        let claude_dir = home.join(".claude/projects/-Users-x-Luma");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("sid-1.jsonl"), claude_fixture()).unwrap();
        let sidecar = claude_dir.join("sid-1/subagents");
        fs::create_dir_all(&sidecar).unwrap();
        fs::write(sidecar.join("agent-abc.jsonl"), "{}").unwrap();
        // Codex.
        let codex_dir = home.join(".codex/sessions/2026/11/06");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::write(codex_dir.join("rollout-a.jsonl"), codex_fixture()).unwrap();
        // Gemini.
        let gemini_dir = home.join(".gemini/tmp/k/chats");
        fs::create_dir_all(&gemini_dir).unwrap();
        fs::write(
            gemini_dir.join("session-1.json"),
            r#"{"sessionId":"gem-1","messages":[{"type":"user","content":[{"text":"hi"}]}]}"#,
        )
        .unwrap();
        // An agent that is not installed contributes nothing.
        let empty = discover_in_home(&home).expect("discovers");
        assert_eq!(empty.agents.len(), 3);
        let count = |id: &str| {
            empty
                .agents
                .iter()
                .find(|agent| agent.id == id)
                .map(|agent| agent.session_count)
                .unwrap_or(0)
        };
        assert_eq!(count("claude"), 1);
        assert_eq!(count("codex"), 1);
        assert_eq!(count("gemini"), 1);
        assert_eq!(empty.sessions.len(), 3);
        // Store paths are canonical `/`-separated.
        assert!(empty.sessions.iter().all(|s| !s.source_path.contains('\\')));
    }

    // ── Deletion ──

    fn init_db(tag: &str) -> PathBuf {
        let db = temp_root(tag).join("test.sqlite3");
        database::initialize(&db).unwrap();
        db
    }

    #[test]
    fn trash_requires_fresh_discovery_and_matching_size() {
        let home = temp_root("trash-verify");
        let claude_dir = home.join(".claude/projects/-Users-x-repo");
        fs::create_dir_all(&claude_dir).unwrap();
        let transcript = claude_dir.join("sid-1.jsonl");
        fs::write(&transcript, claude_fixture()).unwrap();
        let db_path = init_db("trash-verify-db");

        // Unknown path is out of scope.
        let report = trash_sessions_in(
            &db_path,
            &home,
            &[CleanRequestItem {
                path: "/somewhere/else.jsonl".into(),
                size_bytes: 1,
            }],
            &|_| Ok(()),
        )
        .expect("runs");
        assert!(report.succeeded.is_empty());
        assert!(report.failed.iter().any(|f| f.reason.contains("重新扫描")));

        // Size mismatch is rejected.
        let store = to_store_path(&transcript);
        let report = trash_sessions_in(
            &db_path,
            &home,
            &[CleanRequestItem {
                path: store.clone(),
                size_bytes: 999,
            }],
            &|_| Ok(()),
        )
        .expect("runs");
        assert!(report.failed.iter().any(|f| f.reason.contains("不一致")));
    }

    #[test]
    fn trash_moves_transcript_and_sidecars() {
        let home = temp_root("trash-exec");
        let claude_dir = home.join(".claude/projects/-Users-x-repo");
        fs::create_dir_all(&claude_dir).unwrap();
        let transcript = claude_dir.join("sid-1.jsonl");
        fs::write(&transcript, claude_fixture()).unwrap();
        fs::create_dir_all(claude_dir.join("sid-1")).unwrap();
        fs::create_dir_all(home.join(".claude/file-history/sid-1")).unwrap();
        let db_path = init_db("trash-exec-db");

        let trashed: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());
        let report = trash_sessions_in(
            &db_path,
            &home,
            &[CleanRequestItem {
                path: to_store_path(&transcript),
                size_bytes: fs::metadata(&transcript).unwrap().len(),
            }],
            &|native: &Path| {
                trashed.borrow_mut().push(native.to_path_buf());
                Ok(())
            },
        )
        .expect("runs");

        assert_eq!(report.succeeded.len(), 1);
        assert!(report.failed.is_empty());
        // Transcript + sidecar dir + file-history dir, in that order.
        let trashed = trashed.into_inner();
        assert!(trashed[0].ends_with("sid-1.jsonl"));
        assert!(trashed.iter().any(|p| p.ends_with("sid-1")));
        assert!(trashed
            .iter()
            .any(|p| p.to_string_lossy().contains("file-history/sid-1")));
    }

    #[test]
    fn trash_honours_whitelist_fail_closed() {
        let home = temp_root("trash-whitelist");
        let claude_dir = home.join(".claude/projects/-Users-x-repo");
        fs::create_dir_all(&claude_dir).unwrap();
        let transcript = claude_dir.join("sid-1.jsonl");
        fs::write(&transcript, claude_fixture()).unwrap();
        let db_path = init_db("trash-whitelist-db");
        // Whitelist the transcript's directory.
        let connection = database::open(&db_path).unwrap();
        connection
            .execute(
                "INSERT INTO cleaner_whitelist (path, created_at) VALUES (?1, ?2)",
                rusqlite::params![to_store_path(&claude_dir), 0],
            )
            .unwrap();

        let report = trash_sessions_in(
            &db_path,
            &home,
            &[CleanRequestItem {
                path: to_store_path(&transcript),
                size_bytes: fs::metadata(&transcript).unwrap().len(),
            }],
            &|_| Ok(()),
        )
        .expect("runs");
        assert!(report.succeeded.is_empty());
        assert!(report.failed.iter().any(|f| f.reason.contains("白名单")));
    }
}
