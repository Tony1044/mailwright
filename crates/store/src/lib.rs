//! MailCraft's offline cache, one SQLite file per mailbox.
//!
//! The sync engine writes what Graph sends (folders, message lists from delta rounds, bodies,
//! attachments, calendar windows); the interface reads from here, so everything already fetched
//! is there with no network. Changes made in the interface are applied here at once and queued
//! ([`Action`]) until the server has them; while a message has a change queued, sync doesn't
//! overwrite it.
//!
//! It's a cache: if the schema changes between versions, the file is rebuilt from the server.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod model;

use std::path::Path;

use mailcraft_graph::types::{Attachment, Calendar, Event, MailFolder, Message};
use rusqlite::{Connection, OptionalExtension, Row, params};

pub use model::*;

const SCHEMA_VERSION: i64 = 3;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the offline cache: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("the offline cache: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    db: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE folders(
    id TEXT PRIMARY KEY, parent_id TEXT, name TEXT NOT NULL, well_known TEXT,
    unread INTEGER NOT NULL DEFAULT 0, total INTEGER NOT NULL DEFAULT 0,
    delta_link TEXT, synced INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0, ord INTEGER NOT NULL DEFAULT 0);
CREATE TABLE messages(
    rowid INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, folder_id TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '', preview TEXT NOT NULL DEFAULT '',
    from_name TEXT NOT NULL DEFAULT '', from_addr TEXT NOT NULL DEFAULT '',
    to_json TEXT NOT NULL DEFAULT '[]', cc_json TEXT NOT NULL DEFAULT '[]', bcc_json TEXT NOT NULL DEFAULT '[]',
    received INTEGER NOT NULL DEFAULT 0, sent INTEGER NOT NULL DEFAULT 0,
    is_read INTEGER NOT NULL DEFAULT 0, is_draft INTEGER NOT NULL DEFAULT 0, has_att INTEGER NOT NULL DEFAULT 0,
    importance TEXT NOT NULL DEFAULT 'normal', flag TEXT NOT NULL DEFAULT 'notFlagged',
    conversation_id TEXT NOT NULL DEFAULT '', focused INTEGER NOT NULL DEFAULT 1,
    categories TEXT NOT NULL DEFAULT '[]', is_meeting INTEGER NOT NULL DEFAULT 0, meeting_type TEXT, event_id TEXT,
    web_link TEXT NOT NULL DEFAULT '', body TEXT, pending INTEGER NOT NULL DEFAULT 0);
CREATE INDEX messages_by_folder ON messages(folder_id, received DESC);
CREATE INDEX messages_by_conversation ON messages(conversation_id);
CREATE VIRTUAL TABLE messages_fts USING fts5(subject, people, body, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE attachments(
    message_id TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, content_type TEXT NOT NULL,
    size INTEGER NOT NULL, inline INTEGER NOT NULL, content_id TEXT NOT NULL DEFAULT '', data BLOB,
    PRIMARY KEY(message_id, id));
CREATE TABLE calendars(id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL, can_edit INTEGER NOT NULL, is_default INTEGER NOT NULL, ord INTEGER NOT NULL);
CREATE TABLE events(
    id TEXT PRIMARY KEY, calendar_id TEXT NOT NULL, subject TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL,
    all_day INTEGER NOT NULL, location TEXT NOT NULL, organizer_name TEXT NOT NULL, organizer_addr TEXT NOT NULL,
    attendees TEXT NOT NULL, show_as TEXT NOT NULL, response TEXT NOT NULL, is_organizer INTEGER NOT NULL,
    is_cancelled INTEGER NOT NULL, join_url TEXT NOT NULL, kind TEXT NOT NULL, series_master_id TEXT NOT NULL,
    categories TEXT NOT NULL, reminder INTEGER, preview TEXT NOT NULL, body TEXT, web_link TEXT NOT NULL,
    local INTEGER NOT NULL DEFAULT 0, pending INTEGER NOT NULL DEFAULT 0);
CREATE INDEX events_by_time ON events(start, end);
CREATE TABLE actions(seq INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT NOT NULL DEFAULT '');
"#;

/// RFC 3339 / Graph timestamps to Unix seconds.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp());
    }
    // Graph's dateTimeTimeZone: no offset, fractional seconds, in UTC here.
    let s = s.trim_end_matches('Z');
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
        .map(|t| t.and_utc().timestamp())
}

fn addrs_json(list: &[mailcraft_graph::types::Recipient]) -> String {
    let v: Vec<Addr> = list.iter().map(Addr::from_graph).collect();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

fn json_vec<T: serde::de::DeserializeOwned>(s: &str) -> Vec<T> {
    serde_json::from_str(s).unwrap_or_default()
}

const MESSAGE_COLUMNS: &str = "rowid, id, folder_id, subject, preview, from_name, from_addr, to_json, cc_json, bcc_json, received, sent, is_read, is_draft, has_att, importance, flag, conversation_id, focused, categories, is_meeting, meeting_type, event_id, web_link, body, pending";

fn message_row(r: &Row<'_>, with_body: bool) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        rowid: r.get(0)?,
        id: r.get(1)?,
        folder_id: r.get(2)?,
        subject: r.get(3)?,
        preview: r.get(4)?,
        from: Addr { name: r.get(5)?, address: r.get(6)? },
        to: json_vec(&r.get::<_, String>(7)?),
        cc: json_vec(&r.get::<_, String>(8)?),
        bcc: json_vec(&r.get::<_, String>(9)?),
        received: r.get(10)?,
        sent: r.get(11)?,
        is_read: r.get(12)?,
        is_draft: r.get(13)?,
        has_attachments: r.get(14)?,
        importance: r.get(15)?,
        flag: r.get(16)?,
        conversation_id: r.get(17)?,
        focused: r.get(18)?,
        categories: json_vec(&r.get::<_, String>(19)?),
        is_meeting: r.get(20)?,
        meeting_type: r.get(21)?,
        event_id: r.get(22)?,
        web_link: r.get(23)?,
        body: if with_body { r.get(24)? } else { None },
        pending: r.get::<_, i64>(25)? > 0,
    })
}

const EVENT_COLUMNS: &str = "id, calendar_id, subject, start, end, all_day, location, organizer_name, organizer_addr, attendees, show_as, response, is_organizer, is_cancelled, join_url, kind, series_master_id, categories, reminder, preview, body, web_link, local";

fn event_row(r: &Row<'_>) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        id: r.get(0)?,
        calendar_id: r.get(1)?,
        subject: r.get(2)?,
        start: r.get(3)?,
        end: r.get(4)?,
        all_day: r.get(5)?,
        location: r.get(6)?,
        organizer: Addr { name: r.get(7)?, address: r.get(8)? },
        attendees: json_vec(&r.get::<_, String>(9)?),
        show_as: r.get(10)?,
        response: r.get(11)?,
        is_organizer: r.get(12)?,
        is_cancelled: r.get(13)?,
        join_url: r.get(14)?,
        kind: r.get(15)?,
        series_master_id: r.get(16)?,
        categories: json_vec(&r.get::<_, String>(17)?),
        reminder_minutes: r.get(18)?,
        preview: r.get(19)?,
        body: r.get(20)?,
        web_link: r.get(21)?,
        local: r.get(22)?,
    })
}

/// Turn what someone typed into an FTS5 query: every word must appear, as a prefix.
fn fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '\'')).collect::<String>())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Plain text from HTML, for the search index (rough is fine).
pub fn index_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let lower = html.to_ascii_lowercase();
    let mut skip_until: Option<&str> = None;
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = skip_until {
            if lower.as_bytes()[i..].starts_with(end.as_bytes()) {
                skip_until = None;
                i += end.len();
            } else {
                i += 1;
            }
            continue;
        }
        let c = bytes[i];
        if c == b'<' {
            if lower.as_bytes()[i..].starts_with(b"<style") {
                skip_until = Some("</style>");
            } else if lower.as_bytes()[i..].starts_with(b"<script") {
                skip_until = Some("</script>");
            }
            in_tag = true;
        } else if c == b'>' {
            in_tag = false;
            out.push(' ');
        } else if !in_tag {
            // Copy whole UTF-8 sequences.
            let len = match c {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                _ => 4,
            };
            if let Some(s) = html.get(i..i + len) {
                out.push_str(s);
            }
            i += len;
            continue;
        }
        i += 1;
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let db = Connection::open(path)?;
        Self::init(db)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Store> {
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        db.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            // Two connections (the interface and the sync engine) may open a new file at once:
            // one builds the schema while the other waits, then finds it built.
            db.execute_batch("BEGIN IMMEDIATE")?;
            let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if version == SCHEMA_VERSION {
                db.execute_batch("COMMIT")?;
                return Ok(Store { db });
            }
            let r = Self::rebuild(&db);
            db.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
            r?;
        }
        Ok(Store { db })
    }

    fn rebuild(db: &Connection) -> Result<()> {
        {
            // A cache: start again rather than migrate.
            let tables: Vec<String> = {
                let mut st = db.prepare("SELECT name FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '%fts_%'")?;
                st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            // Keep queued changes across a rebuild.
            let pending: Vec<String> = if tables.iter().any(|t| t == "actions") {
                let mut st = db.prepare("SELECT payload FROM actions ORDER BY seq")?;
                st.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect()
            } else {
                Vec::new()
            };
            for t in tables {
                db.execute_batch(&format!("DROP TABLE IF EXISTS \"{t}\";"))?;
            }
            db.execute_batch(SCHEMA)?;
            for p in pending {
                db.execute("INSERT INTO actions(payload) VALUES (?1)", [p])?;
            }
            db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    // ---- key/value ---------------------------------------------------------------------------

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute("INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
        Ok(())
    }

    // ---- folders -----------------------------------------------------------------------------

    /// Replace the folder list with the server's (depth-first order), keeping each folder's delta
    /// link. Folders that are gone take their messages with them.
    pub fn save_folders(&mut self, folders: &[MailFolder], well_known: &[(String, String)]) -> Result<()> {
        let tx = self.db.transaction()?;
        let ids: Vec<&str> = folders.iter().map(|f| f.id.as_str()).collect();
        {
            let existing: Vec<String> = {
                let mut st = tx.prepare("SELECT id FROM folders")?;
                st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for old in existing {
                if !ids.contains(&old.as_str()) && !old.starts_with("local:") {
                    tx.execute("DELETE FROM messages_fts WHERE rowid IN (SELECT rowid FROM messages WHERE folder_id = ?1)", [&old])?;
                    tx.execute("DELETE FROM messages WHERE folder_id = ?1 AND pending = 0", [&old])?;
                    tx.execute("DELETE FROM folders WHERE id = ?1", [&old])?;
                }
            }
            let mut up = tx.prepare(
                "INSERT INTO folders(id, parent_id, name, well_known, unread, total, hidden, ord) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET parent_id = excluded.parent_id, name = excluded.name, well_known = excluded.well_known,
                   unread = excluded.unread, total = excluded.total, hidden = excluded.hidden, ord = excluded.ord",
            )?;
            for (i, f) in folders.iter().enumerate() {
                let wk = well_known.iter().find(|(_, id)| *id == f.id).map(|(name, _)| name.clone());
                let parent = f.parent_folder_id.clone().filter(|p| ids.contains(&p.as_str()));
                up.execute(params![f.id, parent, f.display_name, wk, f.unread_item_count, f.total_item_count, f.is_hidden.unwrap_or(false), i as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<FolderRow>> {
        let mut st = self.db.prepare("SELECT id, parent_id, name, well_known, unread, total, synced, hidden FROM folders ORDER BY ord")?;
        let rows = st.query_map([], |r| {
            Ok(FolderRow {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                name: r.get(2)?,
                well_known: r.get(3)?,
                unread: r.get(4)?,
                total: r.get(5)?,
                synced: r.get(6)?,
                hidden: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn folder_by_well_known(&self, name: &str) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT id FROM folders WHERE well_known = ?1", [name], |r| r.get(0)).optional()?)
    }

    pub fn delta_link(&self, folder: &str) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT delta_link FROM folders WHERE id = ?1", [folder], |r| r.get(0)).optional()?.flatten())
    }

    /// The end of a delta round: remember where to carry on, and that the folder is in the cache.
    pub fn set_delta_link(&self, folder: &str, link: Option<&str>) -> Result<()> {
        self.db.execute("UPDATE folders SET delta_link = ?2, synced = ?3 WHERE id = ?1", params![folder, link, link.is_some()])?;
        Ok(())
    }

    /// Recount a folder's unread and total from the cache (after local changes).
    pub fn recount(&self, folder: &str) -> Result<()> {
        self.db.execute(
            "UPDATE folders SET unread = (SELECT COUNT(*) FROM messages WHERE folder_id = ?1 AND is_read = 0),
                                total = MAX(total, (SELECT COUNT(*) FROM messages WHERE folder_id = ?1)) WHERE id = ?1 AND synced = 1",
            [folder],
        )?;
        Ok(())
    }

    fn bump_counts(&self, folder: &str, unread: i64, total: i64) -> Result<()> {
        self.db.execute("UPDATE folders SET unread = MAX(0, unread + ?2), total = MAX(0, total + ?3) WHERE id = ?1", params![folder, unread, total])?;
        Ok(())
    }

    // ---- messages ----------------------------------------------------------------------------

    /// Save messages from a delta page or search. Messages with a change waiting keep their local
    /// state; bodies already fetched are kept unless the message changed on the server.
    pub fn upsert_messages(&mut self, folder: &str, msgs: &[Message]) -> Result<usize> {
        let tx = self.db.transaction()?;
        let mut n = 0;
        for m in msgs {
            let pending: Option<i64> = tx.query_row("SELECT pending FROM messages WHERE id = ?1", [&m.id], |r| r.get(0)).optional()?;
            if pending.unwrap_or(0) > 0 {
                continue;
            }
            let folder_id = m.parent_folder_id.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| folder.to_string());
            let from = m.from.as_ref().or(m.sender.as_ref()).map(Addr::from_graph).unwrap_or_default();
            let received = m.received_date_time.as_deref().and_then(parse_time).unwrap_or(0);
            let sent = m.sent_date_time.as_deref().and_then(parse_time).unwrap_or(received);
            let categories = serde_json::to_string(&m.categories)?;
            tx.execute(
                "INSERT INTO messages(id, folder_id, subject, preview, from_name, from_addr, to_json, cc_json, bcc_json, received, sent,
                    is_read, is_draft, has_att, importance, flag, conversation_id, focused, categories, is_meeting, meeting_type, web_link)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
                 ON CONFLICT(id) DO UPDATE SET folder_id = excluded.folder_id, subject = excluded.subject, preview = excluded.preview,
                    from_name = excluded.from_name, from_addr = excluded.from_addr, to_json = excluded.to_json, cc_json = excluded.cc_json,
                    received = excluded.received, sent = excluded.sent, is_read = excluded.is_read, is_draft = excluded.is_draft,
                    has_att = excluded.has_att, importance = excluded.importance, flag = excluded.flag, conversation_id = excluded.conversation_id,
                    focused = excluded.focused, categories = excluded.categories, is_meeting = excluded.is_meeting,
                    meeting_type = COALESCE(excluded.meeting_type, messages.meeting_type), web_link = excluded.web_link,
                    body = CASE WHEN messages.is_draft = 1 OR messages.preview <> excluded.preview THEN NULL ELSE messages.body END",
                params![
                    m.id,
                    folder_id,
                    m.subject.clone().unwrap_or_default(),
                    m.body_preview.clone().unwrap_or_default(),
                    from.name,
                    from.address,
                    addrs_json(&m.to_recipients),
                    addrs_json(&m.cc_recipients),
                    addrs_json(&m.bcc_recipients),
                    received,
                    sent,
                    m.is_read.unwrap_or(true),
                    m.is_draft.unwrap_or(false),
                    m.has_attachments.unwrap_or(false),
                    m.importance.clone().unwrap_or_else(|| "normal".into()),
                    m.flag.as_ref().map(|f| f.flag_status.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "notFlagged".into()),
                    m.conversation_id.clone().unwrap_or_default(),
                    m.inference_classification.as_deref() != Some("other"),
                    categories,
                    m.is_meeting_message(),
                    m.meeting_message_type,
                    m.web_link.clone().unwrap_or_default(),
                ],
            )?;
            let (rowid, body): (i64, Option<String>) = tx.query_row("SELECT rowid, body FROM messages WHERE id = ?1", [&m.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let people = format!(
                "{} {} {}",
                from.full(),
                m.to_recipients.iter().map(|r| Addr::from_graph(r).full()).collect::<Vec<_>>().join(" "),
                m.cc_recipients.iter().map(|r| Addr::from_graph(r).full()).collect::<Vec<_>>().join(" ")
            );
            let text = body.as_deref().map(index_text).unwrap_or_else(|| m.body_preview.clone().unwrap_or_default());
            tx.execute("DELETE FROM messages_fts WHERE rowid = ?1", [rowid])?;
            tx.execute("INSERT INTO messages_fts(rowid, subject, people, body) VALUES (?1, ?2, ?3, ?4)", params![rowid, m.subject.clone().unwrap_or_default(), people, text])?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Delta said these are gone from a folder.
    pub fn remove_messages(&mut self, folder: &str, ids: &[String]) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM messages_fts WHERE rowid IN (SELECT rowid FROM messages WHERE id = ?1 AND folder_id = ?2 AND pending = 0)", [id, folder])?;
            tx.execute("DELETE FROM attachments WHERE message_id = ?1 AND NOT EXISTS (SELECT 1 FROM messages WHERE id = ?1 AND (folder_id <> ?2 OR pending > 0))", [id, folder])?;
            tx.execute("DELETE FROM messages WHERE id = ?1 AND folder_id = ?2 AND pending = 0", [id, folder])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A folder's messages, newest first (or as sorted), filtered.
    pub fn messages(&self, folder: &str, filter: ListFilter, sort: SortBy, ascending: bool, me: &str) -> Result<Vec<MessageRow>> {
        let mut sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE folder_id = ?1");
        match filter {
            ListFilter::All => {}
            ListFilter::Unread => sql.push_str(" AND is_read = 0"),
            ListFilter::Flagged => sql.push_str(" AND flag = 'flagged'"),
            ListFilter::Attachments => sql.push_str(" AND has_att = 1"),
            ListFilter::Focused => sql.push_str(" AND focused = 1"),
            ListFilter::Other => sql.push_str(" AND focused = 0"),
            ListFilter::ToMe => sql.push_str(" AND instr(lower(to_json), lower(?2)) > 0"),
        }
        let dir = if ascending { "ASC" } else { "DESC" };
        let order = match sort {
            SortBy::Date => format!("received {dir}"),
            SortBy::From => format!("lower(CASE WHEN from_name <> '' THEN from_name ELSE from_addr END) {dir}, received DESC"),
            SortBy::Subject => format!("lower(ltrim(replace(replace(replace(subject, 'RE: ', ''), 'FW: ', ''), 'Re: ', ''))) {dir}, received DESC"),
            SortBy::Importance => format!("CASE importance WHEN 'high' THEN 2 WHEN 'normal' THEN 1 ELSE 0 END {dir}, received DESC"),
            SortBy::Flag => format!("CASE flag WHEN 'flagged' THEN 2 WHEN 'complete' THEN 1 ELSE 0 END {dir}, received DESC"),
        };
        sql.push_str(&format!(" ORDER BY {order} LIMIT 20000"));
        let mut st = self.db.prepare(&sql)?;
        let rows = if filter == ListFilter::ToMe {
            st.query_map(params![folder, me], |r| message_row(r, false))?.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            st.query_map(params![folder], |r| message_row(r, false))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    /// Search the cache: in one folder, or everywhere.
    pub fn search(&self, query: &str, folder: Option<&str>, limit: usize) -> Result<Vec<MessageRow>> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let cols = MESSAGE_COLUMNS.split(", ").map(|c| format!("m.{c}")).collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT {cols} FROM messages_fts f JOIN messages m ON m.rowid = f.rowid WHERE messages_fts MATCH ?1 {} ORDER BY m.received DESC LIMIT ?3",
            if folder.is_some() { "AND m.folder_id = ?2" } else { "AND ?2 IS NULL" }
        );
        let mut st = self.db.prepare(&sql)?;
        let rows = st.query_map(params![q, folder, limit as i64], |r| message_row(r, false))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn message(&self, id: &str) -> Result<Option<MessageRow>> {
        Ok(self.db.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"), [id], |r| message_row(r, true)).optional()?)
    }

    /// The other messages in a conversation (any folder), oldest first.
    pub fn conversation(&self, conversation_id: &str) -> Result<Vec<MessageRow>> {
        if conversation_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.db.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 ORDER BY received"))?;
        let rows = st.query_map([conversation_id], |r| message_row(r, false))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A message's full details, from `message_full`.
    pub fn save_body(&mut self, m: &Message) -> Result<()> {
        let html = m.body.as_ref().map(|b| if b.content_type.eq_ignore_ascii_case("html") { b.content.clone() } else { text_to_html(&b.content) }).unwrap_or_default();
        let tx = self.db.transaction()?;
        let rowid: Option<i64> = tx
            .query_row(
                "UPDATE messages SET body = ?2, to_json = ?3, cc_json = ?4, bcc_json = ?5 WHERE id = ?1 RETURNING rowid",
                params![m.id, html, addrs_json(&m.to_recipients), addrs_json(&m.cc_recipients), addrs_json(&m.bcc_recipients)],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(rowid) = rowid {
            tx.execute("UPDATE messages_fts SET body = ?2 WHERE rowid = ?1", params![rowid, index_text(&html)])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_meeting(&self, id: &str, meeting_type: Option<&str>, event_id: Option<&str>) -> Result<()> {
        self.db.execute("UPDATE messages SET meeting_type = COALESCE(?2, meeting_type), event_id = ?3, is_meeting = 1 WHERE id = ?1", params![id, meeting_type, event_id])?;
        Ok(())
    }

    /// Messages in these folders received since `since` whose bodies aren't cached yet, newest first.
    pub fn missing_bodies(&self, folders: &[String], since: i64, limit: usize) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut st = self.db.prepare("SELECT id FROM messages WHERE folder_id = ?1 AND body IS NULL AND received >= ?2 AND id NOT LIKE 'local:%' ORDER BY received DESC LIMIT ?3")?;
        for f in folders {
            let ids = st.query_map(params![f, since, limit as i64], |r| r.get::<_, String>(0))?;
            for id in ids {
                out.push(id?);
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    // ---- attachments -------------------------------------------------------------------------

    pub fn save_attachments(&mut self, message_id: &str, list: &[Attachment]) -> Result<()> {
        let tx = self.db.transaction()?;
        let keep: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        {
            let existing: Vec<String> = {
                let mut st = tx.prepare("SELECT id FROM attachments WHERE message_id = ?1")?;
                st.query_map([message_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for e in existing {
                if !keep.contains(&e.as_str()) {
                    tx.execute("DELETE FROM attachments WHERE message_id = ?1 AND id = ?2", [message_id, &e])?;
                }
            }
            let mut st = tx.prepare(
                "INSERT INTO attachments(message_id, id, name, content_type, size, inline, content_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(message_id, id) DO UPDATE SET name = excluded.name, content_type = excluded.content_type, size = excluded.size,
                   inline = excluded.inline, content_id = CASE WHEN excluded.content_id <> '' THEN excluded.content_id ELSE attachments.content_id END",
            )?;
            for a in list {
                st.execute(params![
                    message_id,
                    a.id,
                    a.name.clone().unwrap_or_else(|| "attachment".into()),
                    a.content_type.clone().unwrap_or_else(|| "application/octet-stream".into()),
                    a.size,
                    a.is_inline,
                    a.content_id.clone().unwrap_or_default().trim_matches(['<', '>']).to_string(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn attachments(&self, message_id: &str) -> Result<Vec<AttachmentRow>> {
        let mut st = self.db.prepare("SELECT message_id, id, name, content_type, size, inline, content_id FROM attachments WHERE message_id = ?1 ORDER BY inline, name")?;
        let rows = st.query_map([message_id], |r| {
            Ok(AttachmentRow { message_id: r.get(0)?, id: r.get(1)?, name: r.get(2)?, content_type: r.get(3)?, size: r.get(4)?, inline: r.get(5)?, content_id: r.get(6)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn has_attachment_list(&self, message_id: &str) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM attachments WHERE message_id = ?1)", [message_id], |r| r.get(0))?)
    }

    pub fn save_attachment_data(&self, message_id: &str, id: &str, data: &[u8], content_id: Option<&str>) -> Result<()> {
        self.db.execute(
            "UPDATE attachments SET data = ?3, content_id = COALESCE(NULLIF(?4, ''), content_id) WHERE message_id = ?1 AND id = ?2",
            params![message_id, id, data, content_id.map(|c| c.trim_matches(['<', '>']).to_string())],
        )?;
        Ok(())
    }

    pub fn attachment_data(&self, message_id: &str, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.db.query_row("SELECT data FROM attachments WHERE message_id = ?1 AND id = ?2", [message_id, id], |r| r.get(0)).optional()?.flatten())
    }

    /// Inline images by content id (for `cid:` links in the body), where downloaded.
    pub fn inline_images(&self, message_id: &str) -> Result<Vec<(String, String, Vec<u8>)>> {
        let mut st = self.db.prepare("SELECT content_id, content_type, data FROM attachments WHERE message_id = ?1 AND data IS NOT NULL AND content_id <> ''")?;
        let rows = st.query_map([message_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Inline attachments not yet downloaded.
    pub fn missing_inline(&self, message_id: &str) -> Result<Vec<String>> {
        let mut st = self.db.prepare("SELECT id FROM attachments WHERE message_id = ?1 AND inline = 1 AND data IS NULL AND size <= 4194304")?;
        let rows = st.query_map([message_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- calendar ----------------------------------------------------------------------------

    pub fn save_calendars(&mut self, list: &[Calendar]) -> Result<()> {
        let tx = self.db.transaction()?;
        let keep: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
        let existing: Vec<String> = {
            let mut st = tx.prepare("SELECT id FROM calendars")?;
            st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for e in existing {
            if !keep.contains(&e.as_str()) {
                tx.execute("DELETE FROM calendars WHERE id = ?1", [&e])?;
                tx.execute("DELETE FROM events WHERE calendar_id = ?1", [&e])?;
            }
        }
        for (i, c) in list.iter().enumerate() {
            tx.execute(
                "INSERT INTO calendars(id, name, color, can_edit, is_default, ord) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, color = excluded.color, can_edit = excluded.can_edit, is_default = excluded.is_default, ord = excluded.ord",
                params![c.id, c.name, c.hex_color.clone().unwrap_or_default(), c.can_edit.unwrap_or(true), c.is_default_calendar.unwrap_or(false), i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn calendars(&self) -> Result<Vec<CalendarRow>> {
        let mut st = self.db.prepare("SELECT id, name, color, can_edit, is_default FROM calendars ORDER BY is_default DESC, ord")?;
        let rows = st.query_map([], |r| Ok(CalendarRow { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, can_edit: r.get(3)?, is_default: r.get(4)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replace a calendar's events overlapping `[from, to)` with the server's view of that window.
    pub fn save_calendar_window(&mut self, calendar_id: &str, from: i64, to: i64, events: &[Event]) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM events WHERE calendar_id = ?1 AND start < ?3 AND end > ?2 AND local = 0 AND pending = 0", params![calendar_id, from, to])?;
        for e in events {
            let pending: Option<i64> = tx.query_row("SELECT pending FROM events WHERE id = ?1", [&e.id], |r| r.get(0)).optional()?;
            if pending.unwrap_or(0) > 0 {
                continue;
            }
            upsert_event(&tx, calendar_id, e, false)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save_event(&self, calendar_id: &str, e: &Event) -> Result<()> {
        upsert_event(&self.db, calendar_id, e, false)
    }

    /// Events overlapping `[from, to)`, by start time.
    pub fn events(&self, from: i64, to: i64) -> Result<Vec<EventRow>> {
        let mut st = self.db.prepare(&format!("SELECT {EVENT_COLUMNS} FROM events WHERE start < ?2 AND end > ?1 AND is_cancelled = 0 ORDER BY all_day DESC, start, end DESC"))?;
        let rows = st.query_map(params![from, to], event_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn event(&self, id: &str) -> Result<Option<EventRow>> {
        Ok(self.db.query_row(&format!("SELECT {EVENT_COLUMNS} FROM events WHERE id = ?1"), [id], event_row).optional()?)
    }

    pub fn set_event_body(&self, id: &str, html: &str) -> Result<()> {
        self.db.execute("UPDATE events SET body = ?2 WHERE id = ?1", params![id, html])?;
        Ok(())
    }

    /// An event made here, shown until the server has it.
    pub fn insert_local_event(&self, row: &EventRow) -> Result<()> {
        self.db.execute(
            &format!("INSERT OR REPLACE INTO events({EVENT_COLUMNS}, pending) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, 1, 1)"),
            params![
                row.id,
                row.calendar_id,
                row.subject,
                row.start,
                row.end,
                row.all_day,
                row.location,
                row.organizer.name,
                row.organizer.address,
                serde_json::to_string(&row.attendees)?,
                row.show_as,
                row.response,
                row.is_organizer,
                row.is_cancelled,
                row.join_url,
                row.kind,
                row.series_master_id,
                serde_json::to_string(&row.categories)?,
                row.reminder_minutes,
                row.preview,
                row.body,
                row.web_link,
            ],
        )?;
        Ok(())
    }

    // ---- local changes -----------------------------------------------------------------------

    /// Apply a change to the cache and queue it for the server. Returns its queue number.
    pub fn apply(&mut self, action: Action) -> Result<i64> {
        let tx = self.db.transaction()?;
        match &action {
            Action::SetRead { id, read } => {
                let folder: Option<(String, bool)> = tx.query_row("SELECT folder_id, is_read FROM messages WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                if let Some((folder, was)) = folder
                    && was != *read {
                        tx.execute("UPDATE messages SET is_read = ?2 WHERE id = ?1", params![id, read])?;
                        tx.execute("UPDATE folders SET unread = MAX(0, unread + ?2) WHERE id = ?1", params![folder, if *read { -1 } else { 1 }])?;
                    }
            }
            Action::SetFlag { id, status } => {
                tx.execute("UPDATE messages SET flag = ?2 WHERE id = ?1", params![id, status])?;
            }
            Action::SetCategories { id, categories } => {
                tx.execute("UPDATE messages SET categories = ?2 WHERE id = ?1", params![id, serde_json::to_string(categories)?])?;
            }
            Action::Move { id, dest } => {
                let from: Option<(String, bool)> = tx.query_row("SELECT folder_id, is_read FROM messages WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                if let Some((from, read)) = from {
                    tx.execute("UPDATE messages SET folder_id = ?2 WHERE id = ?1", params![id, dest])?;
                    let unread = if read { 0 } else { 1 };
                    tx.execute("UPDATE folders SET unread = MAX(0, unread - ?2), total = MAX(0, total - 1) WHERE id = ?1", params![from, unread])?;
                    tx.execute("UPDATE folders SET unread = unread + ?2, total = total + 1 WHERE id = ?1", params![dest, unread])?;
                }
            }
            Action::Delete { id } => {
                let from: Option<(String, bool)> = tx.query_row("SELECT folder_id, is_read FROM messages WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                if let Some((from, read)) = from {
                    tx.execute("UPDATE folders SET unread = MAX(0, unread - ?2), total = MAX(0, total - 1) WHERE id = ?1", params![from, if read { 0 } else { 1 }])?;
                }
                tx.execute("DELETE FROM messages_fts WHERE rowid IN (SELECT rowid FROM messages WHERE id = ?1)", [id])?;
                tx.execute("DELETE FROM attachments WHERE message_id = ?1", [id])?;
                tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
            }
            Action::Send { compose } | Action::SaveDraft { compose } => {
                // The message waits in the Outbox (sending) or Drafts (saving) until the server has it.
                let wk = if matches!(action, Action::Send { .. }) { "outbox" } else { "drafts" };
                let folder: String = tx.query_row("SELECT id FROM folders WHERE well_known = ?1", [wk], |r| r.get(0)).optional()?.unwrap_or_else(|| format!("local:{wk}"));
                if let Some(d) = &compose.draft_id {
                    tx.execute("DELETE FROM messages WHERE id = ?1", [d])?;
                }
                let now = chrono::Utc::now().timestamp();
                let preview: String = index_text(&compose.html).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
                tx.execute(
                    "INSERT OR REPLACE INTO messages(id, folder_id, subject, preview, to_json, cc_json, bcc_json, received, sent, is_read, is_draft, has_att, importance, body, pending)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1, 1, ?9, ?10, ?11, 1)",
                    params![
                        compose.local_id,
                        folder,
                        compose.subject,
                        preview,
                        serde_json::to_string(&compose.to)?,
                        serde_json::to_string(&compose.cc)?,
                        serde_json::to_string(&compose.bcc)?,
                        now,
                        !compose.attachments.is_empty(),
                        compose.importance,
                        compose.html,
                    ],
                )?;
            }
            Action::CreateEvent { .. } => {}
            Action::UpdateEvent { id, patch } => {
                if let Some(s) = patch.get("subject").and_then(|v| v.as_str()) {
                    tx.execute("UPDATE events SET subject = ?2 WHERE id = ?1", params![id, s])?;
                }
                if let Some(s) = patch.get("location").and_then(|v| v.get("displayName")).and_then(|v| v.as_str()) {
                    tx.execute("UPDATE events SET location = ?2 WHERE id = ?1", params![id, s])?;
                }
                if let Some(s) = patch.get("showAs").and_then(|v| v.as_str()) {
                    tx.execute("UPDATE events SET show_as = ?2 WHERE id = ?1", params![id, s])?;
                }
                if let Some(s) = patch.get("isAllDay").and_then(|v| v.as_bool()) {
                    tx.execute("UPDATE events SET all_day = ?2 WHERE id = ?1", params![id, s])?;
                }
                for (key, col) in [("start", "start"), ("end", "end")] {
                    if let Some(t) = patch.get(key).and_then(|v| v.get("dateTime")).and_then(|v| v.as_str()).and_then(parse_time) {
                        tx.execute(&format!("UPDATE events SET {col} = ?2 WHERE id = ?1"), params![id, t])?;
                    }
                }
                if let Some(b) = patch.get("body").and_then(|v| v.get("content")).and_then(|v| v.as_str()) {
                    tx.execute("UPDATE events SET body = ?2, preview = ?3 WHERE id = ?1", params![id, b, index_text(b).split_whitespace().collect::<Vec<_>>().join(" ")])?;
                }
                if let Some(a) = patch.get("attendees") {
                    let list: Vec<mailcraft_graph::types::Attendee> = serde_json::from_value(a.clone()).unwrap_or_default();
                    tx.execute("UPDATE events SET attendees = ?2 WHERE id = ?1", params![id, serde_json::to_string(&attendee_rows(&list))?])?;
                }
                if let Some(m) = patch.get("reminderMinutesBeforeStart").and_then(|v| v.as_i64()) {
                    tx.execute("UPDATE events SET reminder = ?2 WHERE id = ?1", params![id, m])?;
                }
            }
            Action::DeleteEvent { id } | Action::CancelEvent { id, .. } => {
                tx.execute("UPDATE events SET is_cancelled = 1 WHERE id = ?1", [id])?;
            }
            Action::RespondEvent { id, response, .. } => {
                let r = match response.as_str() {
                    "accept" => "accepted",
                    "tentative" => "tentativelyAccepted",
                    _ => "declined",
                };
                tx.execute("UPDATE events SET response = ?2, show_as = CASE WHEN ?2 = 'tentativelyAccepted' THEN 'tentative' ELSE show_as END WHERE id = ?1", params![id, r])?;
                if r == "declined" {
                    tx.execute("UPDATE events SET is_cancelled = 1 WHERE id = ?1", [id])?;
                }
            }
        }
        if let Some(id) = action.message_id() {
            tx.execute("UPDATE messages SET pending = pending + 1 WHERE id = ?1", [id])?;
        }
        if let Action::UpdateEvent { id, .. } | Action::DeleteEvent { id } | Action::CancelEvent { id, .. } | Action::RespondEvent { id, .. } = &action {
            tx.execute("UPDATE events SET pending = pending + 1 WHERE id = ?1", [id])?;
        }
        tx.execute("INSERT INTO actions(payload) VALUES (?1)", [serde_json::to_string(&action)?])?;
        let seq = tx.last_insert_rowid();
        tx.commit()?;
        Ok(seq)
    }

    /// Changes waiting for the server, oldest first.
    pub fn queued(&self) -> Result<Vec<QueuedAction>> {
        let mut st = self.db.prepare("SELECT seq, payload, attempts, last_error FROM actions ORDER BY seq")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, payload, attempts, last_error) = row?;
            match serde_json::from_str::<Action>(&payload) {
                Ok(action) => out.push(QueuedAction { seq, action, attempts, last_error }),
                Err(e) => {
                    log::warn!("dropping unreadable queued change {seq}: {e}");
                    self.db.execute("DELETE FROM actions WHERE seq = ?1", [seq])?;
                }
            }
        }
        Ok(out)
    }

    pub fn queued_count(&self) -> Result<i64> {
        Ok(self.db.query_row("SELECT COUNT(*) FROM actions", [], |r| r.get(0))?)
    }

    /// The server has this change (or it can never be made): take it off the queue.
    pub fn done(&mut self, q: &QueuedAction) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM actions WHERE seq = ?1", [q.seq])?;
        if let Some(id) = q.action.message_id() {
            tx.execute("UPDATE messages SET pending = MAX(0, pending - 1) WHERE id = ?1", [id])?;
        }
        if let Action::UpdateEvent { id, .. } | Action::DeleteEvent { id } | Action::CancelEvent { id, .. } | Action::RespondEvent { id, .. } = &q.action {
            tx.execute("UPDATE events SET pending = MAX(0, pending - 1) WHERE id = ?1", [id])?;
        }
        match &q.action {
            Action::Send { compose } | Action::SaveDraft { compose } => {
                tx.execute("DELETE FROM messages WHERE id = ?1", [&compose.local_id])?;
            }
            Action::Delete { .. } => {}
            Action::DeleteEvent { id } | Action::CancelEvent { id, .. } => {
                tx.execute("DELETE FROM events WHERE id = ?1", [id])?;
            }
            Action::RespondEvent { id, response, .. } if response == "decline" => {
                tx.execute("DELETE FROM events WHERE id = ?1", [id])?;
            }
            Action::CreateEvent { local_id, .. } => {
                tx.execute("DELETE FROM events WHERE id = ?1", [local_id])?;
            }
            _ => {}
        }
        tx.commit()?;
        Ok(())
    }

    pub fn failed(&self, q: &QueuedAction, error: &str) -> Result<()> {
        self.db.execute("UPDATE actions SET attempts = attempts + 1, last_error = ?2 WHERE seq = ?1", params![q.seq, error])?;
        Ok(())
    }

    /// The server gave a message a new id (a move): use it everywhere, including queued changes.
    pub fn rename_message(&mut self, old: &str, new: &str) -> Result<()> {
        if old == new {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        // If delta already brought the new id, drop that copy in favour of ours.
        tx.execute("DELETE FROM messages_fts WHERE rowid IN (SELECT rowid FROM messages WHERE id = ?1)", [new])?;
        tx.execute("DELETE FROM messages WHERE id = ?1", [new])?;
        tx.execute("UPDATE messages SET id = ?2 WHERE id = ?1", [old, new])?;
        tx.execute("UPDATE attachments SET message_id = ?2 WHERE message_id = ?1", [old, new])?;
        let rows: Vec<(i64, String)> = {
            let mut st = tx.prepare("SELECT seq, payload FROM actions")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        let quoted_old = serde_json::to_string(old)?;
        let quoted_new = serde_json::to_string(new)?;
        for (seq, payload) in rows {
            if payload.contains(&quoted_old) {
                tx.execute("UPDATE actions SET payload = ?2 WHERE seq = ?1", params![seq, payload.replace(&quoted_old, &quoted_new)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Forget everything (signing out).
    pub fn clear(&self) -> Result<()> {
        self.db.execute_batch("DELETE FROM messages_fts; DELETE FROM messages; DELETE FROM attachments; DELETE FROM folders; DELETE FROM events; DELETE FROM calendars; DELETE FROM actions; DELETE FROM meta;")?;
        Ok(())
    }

    pub fn adjust_counts(&self, folder: &str, unread: i64, total: i64) -> Result<()> {
        self.bump_counts(folder, unread, total)
    }

    /// The server's counts for a folder.
    pub fn set_counts(&self, folder: &str, unread: i64, total: i64) -> Result<()> {
        self.db.execute("UPDATE folders SET unread = ?2, total = ?3 WHERE id = ?1", params![folder, unread, total])?;
        Ok(())
    }

    /// Replace a queued change (after part of it has reached the server).
    pub fn update_action(&self, seq: i64, action: &Action) -> Result<()> {
        self.db.execute("UPDATE actions SET payload = ?2 WHERE seq = ?1", params![seq, serde_json::to_string(action)?])?;
        Ok(())
    }

    /// Drop a change the server refused, leaving the cache as it is (an unsent message stays in
    /// the Outbox to be fixed and sent again).
    pub fn discard(&mut self, q: &QueuedAction) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM actions WHERE seq = ?1", [q.seq])?;
        if let Some(id) = q.action.message_id() {
            tx.execute("UPDATE messages SET pending = MAX(0, pending - 1) WHERE id = ?1", [id])?;
        }
        if let Action::UpdateEvent { id, .. } | Action::DeleteEvent { id } | Action::CancelEvent { id, .. } | Action::RespondEvent { id, .. } = &q.action {
            tx.execute("UPDATE events SET pending = MAX(0, pending - 1), is_cancelled = 0 WHERE id = ?1", [id])?;
        }
        if let Action::Send { compose } | Action::SaveDraft { compose } = &q.action {
            tx.execute("UPDATE messages SET pending = 0 WHERE id = ?1", [&compose.local_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Addresses seen in the cache matching what's been typed (for completion offline).
    pub fn known_addresses(&self, prefix: &str, limit: usize) -> Result<Vec<Addr>> {
        let like = format!("%{}%", prefix.replace(['%', '_'], ""));
        let mut st = self.db.prepare(
            "SELECT from_name, from_addr, COUNT(*) AS n FROM messages WHERE from_addr <> '' AND (from_name LIKE ?1 OR from_addr LIKE ?1)
             GROUP BY lower(from_addr) ORDER BY n DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![like, limit as i64], |r| Ok(Addr { name: r.get(0)?, address: r.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Folders whose list is cached.
    pub fn synced_folders(&self) -> Result<Vec<String>> {
        let mut st = self.db.prepare("SELECT id FROM folders WHERE synced = 1 ORDER BY ord")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn default_calendar(&self) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT id FROM calendars ORDER BY is_default DESC, ord LIMIT 1", [], |r| r.get(0)).optional()?)
    }
}

fn attendee_rows(list: &[mailcraft_graph::types::Attendee]) -> Vec<AttendeeRow> {
    list.iter()
        .map(|a| AttendeeRow {
            addr: Addr { name: a.email_address.name.clone().unwrap_or_default(), address: a.email_address.address.clone().unwrap_or_default() },
            kind: a.kind.clone().unwrap_or_else(|| "required".into()),
            response: a.status.as_ref().and_then(|s| s.response.clone()).unwrap_or_else(|| "none".into()),
        })
        .collect()
}

fn upsert_event(db: &Connection, calendar_id: &str, e: &Event, local: bool) -> Result<()> {
    let all_day = e.is_all_day.unwrap_or(false);
    let start = e.start.as_ref().and_then(|t| parse_time(&t.date_time)).unwrap_or(0);
    let end = e.end.as_ref().and_then(|t| parse_time(&t.date_time)).unwrap_or(start);
    let organizer = e.organizer.as_ref().map(Addr::from_graph).unwrap_or_default();
    let body = e.body.as_ref().map(|b| if b.content_type.eq_ignore_ascii_case("html") { b.content.clone() } else { text_to_html(&b.content) });
    db.execute(
        &format!(
            "INSERT INTO events({EVENT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
             ON CONFLICT(id) DO UPDATE SET calendar_id = excluded.calendar_id, subject = excluded.subject, start = excluded.start, end = excluded.end,
               all_day = excluded.all_day, location = excluded.location, organizer_name = excluded.organizer_name, organizer_addr = excluded.organizer_addr,
               attendees = excluded.attendees, show_as = excluded.show_as, response = excluded.response, is_organizer = excluded.is_organizer,
               is_cancelled = excluded.is_cancelled, join_url = excluded.join_url, kind = excluded.kind, series_master_id = excluded.series_master_id,
               categories = excluded.categories, reminder = excluded.reminder, preview = excluded.preview,
               body = COALESCE(excluded.body, CASE WHEN events.preview = excluded.preview THEN events.body END), web_link = excluded.web_link, local = excluded.local"
        ),
        params![
            e.id,
            calendar_id,
            e.subject.clone().unwrap_or_default(),
            start,
            end.max(start),
            all_day,
            e.location.as_ref().and_then(|l| l.display_name.clone()).unwrap_or_default(),
            organizer.name,
            organizer.address,
            serde_json::to_string(&attendee_rows(&e.attendees))?,
            e.show_as.clone().unwrap_or_else(|| "busy".into()),
            e.response_status.as_ref().and_then(|r| r.response.clone()).unwrap_or_else(|| "none".into()),
            e.is_organizer.unwrap_or(false),
            e.is_cancelled.unwrap_or(false),
            e.online_meeting.as_ref().and_then(|o| o.join_url.clone()).unwrap_or_default(),
            e.kind.clone().unwrap_or_else(|| "singleInstance".into()),
            e.series_master_id.clone().unwrap_or_default(),
            serde_json::to_string(&e.categories)?,
            if e.is_reminder_on == Some(false) { None } else { e.reminder_minutes_before_start },
            e.body_preview.clone().unwrap_or_default(),
            body,
            e.web_link.clone().unwrap_or_default(),
            local,
        ],
    )?;
    Ok(())
}

/// Plain text as HTML (escaped, line breaks kept).
pub fn text_to_html(text: &str) -> String {
    let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<div style=\"white-space:pre-wrap\">{}</div>", esc.replace("\r\n", "\n").replace('\n', "<br>"))
}

#[cfg(test)]
mod tests;
