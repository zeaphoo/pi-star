use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::session::{SessionInfo, entry_id, entry_type};

const MAX_FILES: usize = 500;
const MAX_RESULTS: usize = 30;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 1024 * 1024;
const TIME_BUDGET: Duration = Duration::from_secs(3);
// 32 Mi characters plus the per-message and per-block allowance below. The
// newest sessions are the ones every keystroke reuses, so eviction follows the
// listing's `modified` order rather than insertion order.
const CACHE_TEXT_CHARS: usize = 32 * 1024 * 1024;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub session: SessionInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    pub block_index: usize,
    #[serde(skip_serializing_if = "is_false")]
    pub title_match: bool,
    pub before: String,
    #[serde(rename = "match")]
    pub match_text: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSearchResponse {
    pub results: Vec<SessionSearchHit>,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
struct SearchBlock {
    block_index: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct SearchMessage {
    entry_id: Option<String>,
    blocks: Vec<SearchBlock>,
}

impl SearchMessage {
    fn text(&self) -> String {
        if self.blocks.len() == 1 {
            return self.blocks[0].text.clone();
        }
        self.blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone)]
struct CachedSession {
    size: u64,
    mtime: Option<SystemTime>,
    modified: String,
    text_chars: usize,
    messages: Arc<Vec<SearchMessage>>,
    capped: bool,
}

#[derive(Default)]
struct SearchCache {
    sessions: HashMap<String, CachedSession>,
    text_chars: usize,
}

static CACHE: Lazy<Mutex<SearchCache>> = Lazy::new(|| Mutex::new(SearchCache::default()));

fn file_stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}

fn drop_cached(cache: &mut SearchCache, path: &str) {
    if let Some(entry) = cache.sessions.remove(path) {
        cache.text_chars = cache.text_chars.saturating_sub(entry.text_chars);
    }
}

fn prune_cache(live: &HashSet<String>) {
    let mut cache = CACHE.lock().expect("search cache poisoned");
    let stale: Vec<String> = cache
        .sessions
        .keys()
        .filter(|path| !live.contains(*path))
        .cloned()
        .collect();
    for path in stale {
        drop_cached(&mut cache, &path);
    }
}

fn cached_scan(path: &str, stamp: Option<(u64, Option<SystemTime>)>) -> Option<CachedSession> {
    let mut cache = CACHE.lock().expect("search cache poisoned");
    let Some(stamp) = stamp else {
        drop_cached(&mut cache, path);
        return None;
    };
    let entry = cache.sessions.get(path)?;
    if entry.size != stamp.0 || entry.mtime != stamp.1 {
        drop_cached(&mut cache, path);
        return None;
    }
    Some(entry.clone())
}

fn remember_messages(
    path: &str,
    stamp: (u64, Option<SystemTime>),
    modified: &str,
    messages: Vec<SearchMessage>,
    capped: bool,
) {
    let text_chars = messages.iter().fold(0usize, |total, message| {
        total
            .saturating_add(64)
            .saturating_add(message.entry_id.as_ref().map_or(0, String::len))
            .saturating_add(message.blocks.iter().fold(0usize, |sum, block| {
                sum.saturating_add(16)
                    .saturating_add(block.text.chars().count())
            }))
    });
    if text_chars > CACHE_TEXT_CHARS {
        return;
    }

    let mut cache = CACHE.lock().expect("search cache poisoned");
    let existing = cache.sessions.get(path).map_or(0, |entry| entry.text_chars);
    let projected = cache
        .text_chars
        .saturating_sub(existing)
        .saturating_add(text_chars);
    if projected > CACHE_TEXT_CHARS {
        // Only sessions older than this one may be dropped: a newer session
        // dropped now would be read again on the next keystroke.
        let mut older: Vec<(String, String, usize)> = cache
            .sessions
            .iter()
            .filter(|(candidate, entry)| {
                candidate.as_str() != path && entry.modified.as_str() < modified
            })
            .map(|(candidate, entry)| (candidate.clone(), entry.modified.clone(), entry.text_chars))
            .collect();
        older.sort_by(|left, right| left.1.cmp(&right.1));
        let mut needed = projected - CACHE_TEXT_CHARS;
        let mut count = 0;
        while count < older.len() && needed > 0 {
            needed = needed.saturating_sub(older[count].2);
            count += 1;
        }
        if needed > 0 {
            return;
        }
        for (candidate, _, _) in older.into_iter().take(count) {
            drop_cached(&mut cache, &candidate);
        }
    }
    drop_cached(&mut cache, path);
    cache.text_chars = cache.text_chars.saturating_add(text_chars);
    cache.sessions.insert(
        path.to_string(),
        CachedSession {
            size: stamp.0,
            mtime: stamp.1,
            modified: modified.to_string(),
            text_chars,
            messages: Arc::new(messages),
            capped,
        },
    );
}

#[derive(Default)]
struct ScanFlags {
    truncated: bool,
    capped: bool,
}

fn text_blocks(content: Option<&Value>) -> Vec<SearchBlock> {
    let blocks: Vec<Value> = match content {
        Some(Value::String(text)) => vec![serde_json::json!({"type": "text", "text": text})],
        Some(Value::Array(blocks)) => blocks.clone(),
        _ => Vec::new(),
    };
    blocks
        .into_iter()
        .enumerate()
        .filter_map(|(block_index, block)| {
            if block.get("type").and_then(Value::as_str) != Some("text") {
                return None;
            }
            let text = block.get("text").and_then(Value::as_str)?;
            (!text.is_empty()).then(|| SearchBlock {
                block_index,
                text: text.to_string(),
            })
        })
        .collect()
}

fn read_messages(path: &str, deadline: Instant, flags: &mut ScanFlags) -> Vec<SearchMessage> {
    let Ok(file) = File::open(path) else {
        flags.truncated = true;
        return Vec::new();
    };
    let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    flags.capped = size > MAX_FILE_BYTES;
    let mut reader = BufReader::new(file.take(MAX_FILE_BYTES));
    let mut line = Vec::new();
    let mut messages = Vec::new();

    loop {
        if Instant::now() >= deadline {
            flags.truncated = true;
            break;
        }
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                flags.truncated = true;
                break;
            }
        }
        if line.len() > MAX_LINE_CHARS {
            flags.truncated = true;
            continue;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if entry_type(&entry) != Some("message") {
            continue;
        }
        let message = entry.get("message").unwrap_or(&entry);
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let blocks = text_blocks(message.get("content"));
        if blocks.is_empty() {
            continue;
        }
        messages.push(SearchMessage {
            entry_id: entry_id(&entry),
            blocks,
        });
    }
    messages
}

/// Whitespace separates terms and every term must appear somewhere in the
/// session; a double-quoted run stays one exact phrase. A quote opens a phrase
/// only at the start of a term, so `foo"bar` stays literal; an unclosed quote
/// reads to the end of the query.
fn parse_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        if ch == '"' && (quoted || current.is_empty()) {
            if quoted && !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
            if quoted {
                current.clear();
            }
            quoted = !quoted;
        } else if !quoted && ch.is_whitespace() {
            if !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    let mut unique = Vec::with_capacity(terms.len());
    for term in terms {
        if !unique.contains(&term) {
            unique.push(term);
        }
    }
    unique
}

fn escape_terms(terms: &[String]) -> Result<Vec<Regex>, regex::Error> {
    terms
        .iter()
        .map(|term| Regex::new(&format!("(?i){}", regex::escape(term))))
        .collect()
}

fn fold_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn chars_before(text: &str, end: usize, max: usize) -> (usize, bool) {
    let mut start = end;
    for _ in 0..max {
        let Some((index, _)) = text[..start].char_indices().next_back() else {
            return (0, false);
        };
        start = index;
    }
    (start, start > 0)
}

fn chars_after(text: &str, start: usize, max: usize) -> (usize, bool) {
    let mut end = start;
    for _ in 0..max {
        let Some(ch) = text[end..].chars().next() else {
            return (end, false);
        };
        end += ch.len_utf8();
    }
    (end, end < text.len())
}

fn snippet(
    text: &str,
    start: usize,
    end: usize,
    blocks: &[SearchBlock],
) -> (usize, String, String, String) {
    let mut block_offset = 0usize;
    let mut block_index = blocks.first().map_or(0, |block| block.block_index);
    for block in blocks {
        let block_end = block_offset.saturating_add(block.text.len());
        if start <= block_end {
            block_index = block.block_index;
            break;
        }
        block_offset = block_end.saturating_add(1);
    }

    let (before_start, more_before) = chars_before(text, start, 80);
    let before = fold_whitespace(&text[before_start..start]);
    let before = if more_before {
        format!("...{before}")
    } else {
        before
    };
    let (after_end, more_after) = chars_after(text, end, 80);
    let mut after = fold_whitespace(&text[end..after_end]);
    if more_after {
        after.push_str("...");
    }
    (
        block_index,
        before,
        fold_whitespace(&text[start..end]),
        after,
    )
}

struct Matcher<'a> {
    session: &'a SessionInfo,
    patterns: &'a [Regex],
    first_hit: Option<(usize, String, String, String, Option<String>)>,
    pending: Vec<usize>,
}

impl<'a> Matcher<'a> {
    fn new(session: &'a SessionInfo, patterns: &'a [Regex]) -> Self {
        Self {
            session,
            patterns,
            first_hit: None,
            pending: (1..patterns.len()).collect(),
        }
    }

    /// Returns true when the first term has a hit and every other term has been
    /// seen somewhere in the session.
    fn step(&mut self, message: &SearchMessage) -> bool {
        let text = message.text();
        for index in self.pending.clone() {
            if self.patterns[index].is_match(&text) {
                self.pending.retain(|pending| *pending != index);
            }
        }
        if self.first_hit.is_none()
            && let Some(found) = self.patterns[0].find(&text)
        {
            let (block_index, before, match_text, after) =
                snippet(&text, found.start(), found.end(), &message.blocks);
            self.first_hit = Some((
                block_index,
                before,
                match_text,
                after,
                message.entry_id.clone(),
            ));
        }
        self.first_hit.is_some() && self.pending.is_empty()
    }

    fn result(self) -> Option<SessionSearchHit> {
        let (block_index, before, match_text, after, entry_id) = self.first_hit?;
        if !self.pending.is_empty() {
            return None;
        }
        Some(SessionSearchHit {
            session: self.session.clone(),
            entry_id,
            block_index,
            title_match: false,
            before,
            match_text,
            after,
        })
    }
}

fn title_hit(session: &SessionInfo, patterns: &[Regex]) -> Option<SessionSearchHit> {
    let title = session
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&session.first_message);
    if title.is_empty() || !patterns.iter().all(|pattern| pattern.is_match(title)) {
        return None;
    }
    let found = patterns[0].find(title)?;
    Some(SessionSearchHit {
        session: session.clone(),
        entry_id: None,
        block_index: 0,
        title_match: true,
        before: title[..found.start()].to_string(),
        match_text: found.as_str().to_string(),
        after: title[found.end()..].to_string(),
    })
}

pub fn search_sessions(
    sessions: Vec<SessionInfo>,
    query: &str,
) -> Result<SessionSearchResponse, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(SessionSearchResponse {
            results: Vec::new(),
            truncated: false,
        });
    }
    if query.chars().count() > 200 {
        return Err("Search query exceeds 200 characters".into());
    }
    let terms = parse_terms(query);
    let terms = if terms.is_empty() {
        vec![query.to_string()]
    } else {
        terms
    };
    let patterns = escape_terms(&terms).map_err(|error| error.to_string())?;

    let deadline = Instant::now() + TIME_BUDGET;
    let mut candidates: Vec<SessionInfo> = sessions
        .into_iter()
        .filter(|session| session.transient != Some(true) && !session.path.is_empty())
        .collect();
    candidates.sort_by(|left, right| right.modified.cmp(&left.modified));

    let scoped: Vec<SessionInfo> = candidates.iter().take(MAX_FILES).cloned().collect();
    let live: HashSet<String> = scoped.iter().map(|session| session.path.clone()).collect();
    prune_cache(&live);

    let mut response = SessionSearchResponse {
        results: Vec::new(),
        truncated: candidates.len() > scoped.len(),
    };

    for session in scoped {
        if response.results.len() >= MAX_RESULTS || Instant::now() >= deadline {
            response.truncated = true;
            break;
        }
        let mut matcher = Matcher::new(&session, &patterns);
        let stamp = file_stamp(Path::new(&session.path));
        let cached = cached_scan(&session.path, stamp);
        let mut found = false;
        let mut tail_unread = false;

        if let Some(cached) = cached {
            tail_unread = cached.capped;
            for message in cached.messages.iter() {
                if Instant::now() >= deadline {
                    response.truncated = true;
                    break;
                }
                if matcher.step(message) {
                    found = true;
                    break;
                }
            }
        } else if let Some(stamp) = stamp {
            let mut flags = ScanFlags::default();
            let messages = read_messages(&session.path, deadline, &mut flags);
            tail_unread = flags.capped;
            if flags.truncated {
                response.truncated = true;
            }
            for message in &messages {
                if Instant::now() >= deadline {
                    response.truncated = true;
                    break;
                }
                if matcher.step(message) {
                    found = true;
                    break;
                }
            }
            if !found && !flags.truncated {
                // The stamp was taken before the read: an append that lands
                // during it belongs to the next scan, not to this cache entry.
                remember_messages(
                    &session.path,
                    stamp,
                    &session.modified,
                    messages,
                    flags.capped,
                );
            }
        } else {
            response.truncated = true;
        }

        if found {
            if let Some(hit) = matcher.result() {
                response.results.push(hit);
            }
        } else if tail_unread {
            response.truncated = true;
        }

        if !found
            && Instant::now() < deadline
            && let Some(hit) = title_hit(&session, &patterns)
        {
            response.results.push(hit);
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn session(path: &str, name: Option<&str>, first: &str, modified: &str) -> SessionInfo {
        SessionInfo {
            path: path.into(),
            id: path.into(),
            cwd: "/tmp".into(),
            name: name.map(str::to_owned),
            created: modified.into(),
            modified: modified.into(),
            message_count: 0,
            first_message: first.into(),
            details_pending: None,
            parent_session_id: None,
            relation: None,
            project_root: None,
            project_key: None,
            branch: None,
            is_worktree: None,
            transient: None,
        }
    }

    fn fixture(entries: &[Value]) -> (tempfile::TempDir, SessionInfo) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = File::create(&path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
        let info = session(&path.to_string_lossy(), None, "first", "2026-01-01");
        (dir, info)
    }

    fn message(role: &str, text: &str) -> Value {
        serde_json::json!({"type": "message", "id": format!("id-{role}-{text}"), "message": {"role": role, "content": text}})
    }

    #[test]
    fn parses_quoted_terms_and_full_width_space() {
        assert_eq!(parse_terms("alpha beta"), vec!["alpha", "beta"]);
        assert_eq!(parse_terms("alpha\u{3000}beta"), vec!["alpha", "beta"]);
        assert_eq!(
            parse_terms("\"alpha beta\" gamma"),
            vec!["alpha beta", "gamma"]
        );
        assert_eq!(parse_terms("foo\"bar"), vec!["foo\"bar"]);
        assert_eq!(parse_terms("\"alpha beta"), vec!["alpha beta"]);
        assert_eq!(parse_terms("\"\""), Vec::<String>::new());
    }

    #[test]
    fn matches_every_term_anywhere_and_uses_the_first_terms_hit() {
        let (_dir, info) = fixture(&[
            message("user", "alpha first"),
            message("assistant", "then BETA"),
        ]);
        let response = search_sessions(vec![info], "alpha beta").unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].match_text, "alpha");
        assert_eq!(
            response.results[0].entry_id.as_deref(),
            Some("id-user-alpha first")
        );
        assert!(!response.truncated);
        assert!(
            search_sessions(
                vec![session("/missing", None, "", "2026-01-01")],
                "alpha beta"
            )
            .unwrap()
            .results
            .is_empty()
        );
    }

    #[test]
    fn quotes_are_literal_and_unicode_snippets_are_safe() {
        let (_dir, info) = fixture(&[message(
            "user",
            &format!("{}İ 中文 NEEDLE {}", "x".repeat(200), "y".repeat(200)),
        )]);
        let response = search_sessions(vec![info.clone()], "\"中文 NEEDLE\"").unwrap();
        assert_eq!(response.results[0].match_text, "中文 NEEDLE");
        assert!(response.results[0].before.starts_with("..."));
        assert!(response.results[0].after.ends_with("..."));
        assert!(search_sessions(vec![info], "needle").unwrap().results[0].match_text == "NEEDLE");
    }

    #[test]
    fn falls_back_to_the_title_only_when_every_term_is_there() {
        let (_dir, info) = fixture(&[message("user", "body only")]);
        let info = session(
            &info.path,
            Some("Weekly Planning Notes"),
            "first",
            "2026-01-01",
        );
        let hit = search_sessions(vec![info.clone()], "notes weekly")
            .unwrap()
            .results
            .remove(0);
        assert!(hit.title_match);
        assert!(hit.entry_id.is_none());
        assert_eq!(hit.match_text, "Notes");
        assert!(
            search_sessions(vec![info], "weekly body")
                .unwrap()
                .results
                .is_empty()
        );
    }

    #[test]
    fn reports_truncation_from_file_result_and_candidate_caps() {
        let (_dir, info) = fixture(&[message("user", "needle")]);
        let missing = session("/definitely/missing/session.jsonl", None, "", "2026-01-01");
        assert!(search_sessions(vec![missing], "needle").unwrap().truncated);

        let mut many = Vec::new();
        for index in 0..31 {
            many.push(session(
                &info.path,
                None,
                "",
                &format!("2026-01-{index:02}"),
            ));
        }
        let capped = search_sessions(many, "needle").unwrap();
        assert_eq!(capped.results.len(), 30);
        assert!(capped.truncated);

        let mut too_many = Vec::new();
        for index in 0..501 {
            too_many.push(session(
                &info.path,
                None,
                "",
                &format!("2026-01-{:03}", index % 28 + 1),
            ));
        }
        assert!(search_sessions(too_many, "absent").unwrap().truncated);
    }

    #[test]
    fn bounds_a_single_files_read_and_reports_the_unsearched_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.jsonl");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        let info = session(&path.to_string_lossy(), None, "", "2026-01-01");
        let response = search_sessions(vec![info], "needle").unwrap();
        assert!(response.results.is_empty());
        assert!(response.truncated);
    }

    #[test]
    fn caches_an_unchanged_scan_and_invalidates_it_after_an_append() {
        let (dir, info) = fixture(&[message("user", "before needle")]);
        assert_eq!(
            search_sessions(vec![info.clone()], "needle")
                .unwrap()
                .results
                .len(),
            1
        );
        // The first search stops at its hit, so cache the miss under a new query.
        assert!(
            search_sessions(vec![info.clone()], "absent")
                .unwrap()
                .results
                .is_empty()
        );
        assert!(
            search_sessions(vec![info.clone()], "absent")
                .unwrap()
                .results
                .is_empty()
        );

        let path = dir.path().join("session.jsonl");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&message("assistant", "later needle")).unwrap()
        )
        .unwrap();
        assert_eq!(
            search_sessions(vec![info], "later").unwrap().results.len(),
            1
        );
    }
}
