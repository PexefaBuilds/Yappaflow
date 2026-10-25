//! SQLite storage: settings + dictation log. One connection behind a mutex;
//! all writes are tiny, so contention is irrelevant.

mod migrations;

use crate::settings::Settings;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub struct Storage {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Timings {
    /// Release → recording stopped + resampled + encoded + saved.
    pub prepare_ms: u64,
    pub stt_ms: u64,
    pub cleanup_ms: u64,
    pub insert_ms: u64,
    /// Release → text inserted (the number that matters; target < 1200).
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationRow {
    pub id: i64,
    pub created_at: i64,
    pub app_exe: String,
    pub window_title: String,
    pub mode: String,
    pub duration_ms: i64,
    pub audio_path: Option<String>,
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    pub stt_model: String,
    pub timings: Option<serde_json::Value>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TermRow {
    pub id: i64,
    pub term: String,
    pub sounds_like: String,
    pub source: String,
    pub uses: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnippetRow {
    pub id: i64,
    pub trigger: String,
    pub expansion: String,
    pub uses: i64,
}

pub struct NewDictation<'a> {
    pub app_exe: &'a str,
    pub window_title: &'a str,
    pub mode: &'a str,
    pub duration_ms: u64,
    pub audio_path: &'a str,
    pub stt_provider: &'a str,
    pub stt_model: &'a str,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // journal_mode returns a row, so use the _and_check variant.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        migrations::run(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn load_settings(&self) -> Settings {
        let c = self.conn.lock().unwrap();
        c.query_row("SELECT value FROM settings WHERE key = 'settings'", [], |r| r.get::<_, String>(0))
            .optional()
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO settings(key, value) VALUES('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    pub fn insert_dictation(&self, d: &NewDictation) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO dictations(created_at, app_exe, window_title, mode, duration_ms, audio_path, stt_provider, stt_model, cleanup_provider, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', 'transcribing')",
            params![now_ms(), d.app_exe, d.window_title, d.mode, d.duration_ms as i64, d.audio_path, d.stt_provider, d.stt_model],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn finish_dictation(&self, id: i64, raw: &str, final_text: &str, timings: &Timings, status: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE dictations SET raw_text = ?2, final_text = ?3, timings = ?4, status = ?5, error = NULL WHERE id = ?1",
            params![id, raw, final_text, serde_json::to_string(timings)?, status],
        )?;
        Ok(())
    }

    /// The cleanup/edit call that actually ran, with estimated tokens (for the cost estimate).
    pub fn set_cleanup_usage(&self, id: i64, provider: &str, model: &str, (tokens_in, tokens_out): (i64, i64)) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE dictations SET cleanup_provider = ?2, cleanup_model = ?3, tokens_in = ?4, tokens_out = ?5 WHERE id = ?1",
            params![id, provider, model, tokens_in, tokens_out],
        )?;
        Ok(())
    }

    pub fn set_stt_model(&self, id: i64, provider: &str, model: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE dictations SET stt_provider = ?2, stt_model = ?3 WHERE id = ?1", params![id, provider, model])?;
        Ok(())
    }

    /// Dictations whose transcription succeeded (so the STT call was billed), since `since_ms`.
    pub fn usage_since(&self, since_ms: i64) -> Result<Vec<crate::usage::UsageRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT created_at, duration_ms, stt_provider, stt_model, cleanup_provider, cleanup_model, tokens_in, tokens_out
             FROM dictations WHERE created_at >= ?1 AND raw_text IS NOT NULL ORDER BY created_at",
        )?;
        let rows = st
            .query_map(params![since_ms], |r| {
                Ok(crate::usage::UsageRow {
                    created_at: r.get(0)?,
                    duration_ms: r.get(1)?,
                    stt_provider: r.get(2)?,
                    stt_model: r.get(3)?,
                    cleanup_provider: r.get(4)?,
                    cleanup_model: r.get(5)?,
                    tokens_in: r.get(6)?,
                    tokens_out: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Measured speed per local model since `since_ms`: (model id, dictations, average ms).
    /// Speech is ms per 10 s of audio; cleanup is ms per cleanup that ran (the gate skips some).
    pub fn local_speeds(&self, since_ms: i64) -> Result<Vec<(String, i64, u64)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT stt_model, COUNT(*), CAST(SUM(json_extract(timings, '$.stt_ms')) * 10000.0 / SUM(duration_ms) AS INTEGER)
             FROM dictations WHERE stt_provider = 'whisper_cpp' AND created_at >= ?1 AND timings IS NOT NULL AND duration_ms > 0
             GROUP BY stt_model
             UNION ALL
             SELECT cleanup_model, COUNT(*), CAST(AVG(json_extract(timings, '$.cleanup_ms')) AS INTEGER)
             FROM dictations WHERE cleanup_provider = 'llama_cpp' AND created_at >= ?1 AND json_extract(timings, '$.cleanup_ms') > 0
             GROUP BY cleanup_model",
        )?;
        let rows = st
            .query_map(params![since_ms], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?.max(0) as u64)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn fail_dictation(&self, id: i64, error: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE dictations SET status = 'failed', error = ?2 WHERE id = ?1", params![id, error])?;
        Ok(())
    }

    pub fn style_rules(&self) -> Vec<crate::cleanup::StyleRule> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare("SELECT match_kind, pattern, style FROM app_styles WHERE match_kind != ?1 ORDER BY id") else {
            return Vec::new();
        };
        st.query_map(params![crate::context::RULE_KIND.1], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn list_app_styles(&self) -> Result<Vec<(i64, String, String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, match_kind, pattern, style FROM app_styles WHERE match_kind != ?1 ORDER BY id")?;
        let rows = st
            .query_map(params![crate::context::RULE_KIND.1], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_app_style(&self, kind: &str, pattern: &str, style: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("INSERT INTO app_styles(match_kind, pattern, style) VALUES (?1, ?2, ?3)", params![kind, pattern, style])?;
        Ok(())
    }

    pub fn set_app_style(&self, id: i64, style: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE app_styles SET style = ?2 WHERE id = ?1", params![id, style])?;
        Ok(())
    }

    pub fn list_terms(&self) -> Result<Vec<TermRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, term, sounds_like, source, uses FROM dictionary ORDER BY term COLLATE NOCASE")?;
        let rows = st
            .query_map([], |r| Ok(TermRow { id: r.get(0)?, term: r.get(1)?, sounds_like: r.get(2)?, source: r.get(3)?, uses: r.get(4)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Adding an existing term (any case) updates its "sounds like" instead of failing.
    pub fn add_term(&self, term: &str, sounds_like: &str, source: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO dictionary(term, sounds_like, source, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(term) DO UPDATE SET sounds_like = excluded.sounds_like",
            params![term, sounds_like, source, now_ms()],
        )?;
        Ok(())
    }

    pub fn delete_term(&self, id: i64) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM dictionary WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Terms that appear in a finished dictation get +1 use (most-used terms go to the STT first).
    pub fn bump_terms(&self, text: &str) {
        use crate::snippets::normalize;
        let said = format!(" {} ", normalize(text));
        let hits: Vec<i64> = self
            .list_terms()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| said.contains(&format!(" {} ", normalize(&t.term))))
            .map(|t| t.id)
            .collect();
        let c = self.conn.lock().unwrap();
        for id in hits {
            let _ = c.execute("UPDATE dictionary SET uses = uses + 1 WHERE id = ?1", params![id]);
        }
    }

    pub fn list_snippets(&self) -> Result<Vec<SnippetRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, \"trigger\", expansion, uses FROM snippets ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok(SnippetRow { id: r.get(0)?, trigger: r.get(1)?, expansion: r.get(2)?, uses: r.get(3)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert when `id` is None, else update. Returns the row id.
    pub fn save_snippet(&self, id: Option<i64>, trigger: &str, expansion: &str) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        match id {
            Some(id) => {
                let n = c.execute("UPDATE snippets SET \"trigger\" = ?2, expansion = ?3 WHERE id = ?1", params![id, trigger, expansion])?;
                anyhow::ensure!(n == 1, "That snippet was deleted");
                Ok(id)
            }
            None => {
                c.execute("INSERT INTO snippets(\"trigger\", expansion) VALUES (?1, ?2)", params![trigger, expansion])?;
                Ok(c.last_insert_rowid())
            }
        }
    }

    pub fn delete_snippet(&self, id: i64) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn bump_snippet(&self, id: i64) {
        let _ = self.conn.lock().unwrap().execute("UPDATE snippets SET uses = uses + 1 WHERE id = ?1", params![id]);
    }

    pub fn get(&self, id: i64) -> Result<Option<DictationRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row).optional()?)
    }

    pub fn recent(&self, limit: u32) -> Result<Vec<DictationRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!("{SELECT} ORDER BY id DESC LIMIT ?1"))?;
        let rows = st.query_map(params![limit], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete audio files beyond the newest `keep`; returns paths to remove.
    pub fn take_old_audio(&self, keep: usize) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, audio_path FROM dictations WHERE audio_path IS NOT NULL ORDER BY id DESC LIMIT -1 OFFSET ?1")?;
        let old: Vec<(i64, String)> = st.query_map(params![keep as i64], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (id, _) in &old {
            c.execute("UPDATE dictations SET audio_path = NULL WHERE id = ?1", params![id])?;
        }
        Ok(old.into_iter().map(|(_, p)| p).collect())
    }

    /// Wipes the dictation log. VACUUM so deleted text doesn't linger in free pages.
    /// Deletes every finished row (and rows stuck mid-transcription for 10+ min, e.g. after a crash)
    /// and returns their recordings' paths. A dictation still in flight keeps its row and audio.
    pub fn delete_all_dictations(&self) -> Result<Vec<String>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let cutoff = now_ms() - 10 * 60 * 1000;
        const DONE: &str = "status != 'transcribing' OR created_at < ?1";
        let paths = tx
            .prepare(&format!("SELECT audio_path FROM dictations WHERE audio_path IS NOT NULL AND ({DONE})"))?
            .query_map(params![cutoff], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        tx.execute(&format!("DELETE FROM dictations WHERE {DONE}"), params![cutoff])?;
        tx.commit()?;
        c.execute_batch("VACUUM")?;
        Ok(paths)
    }
}

const SELECT: &str = "SELECT id, created_at, app_exe, window_title, mode, duration_ms, audio_path, raw_text,
    final_text, stt_model, timings, status, error FROM dictations";

fn row(r: &rusqlite::Row) -> rusqlite::Result<DictationRow> {
    let timings: Option<String> = r.get(10)?;
    Ok(DictationRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        app_exe: r.get(2)?,
        window_title: r.get(3)?,
        mode: r.get(4)?,
        duration_ms: r.get(5)?,
        audio_path: r.get(6)?,
        raw_text: r.get(7)?,
        final_text: r.get(8)?,
        stt_model: r.get(9)?,
        timings: timings.and_then(|t| serde_json::from_str(&t).ok()),
        status: r.get(11)?,
        error: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_all_dictations_empties_the_log() {
        let path = std::env::temp_dir().join(format!("yf-del-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Storage::open(&path).unwrap();
        let d = NewDictation {
            app_exe: "a.exe",
            window_title: "",
            mode: "dictate",
            duration_ms: 1,
            audio_path: "x.wav",
            stt_provider: "openai",
            stt_model: "m",
        };
        db.insert_dictation(&d).unwrap();
        let live = db.insert_dictation(&d).unwrap();
        db.finish_dictation(1, "hi", "Hi.", &Timings::default(), "done").unwrap();
        db.set_cleanup_usage(1, "openai", "gpt-6-luna", (100, 5)).unwrap();
        let u = db.usage_since(0).unwrap();
        assert_eq!(u.len(), 1, "only rows with a finished transcription are billed");
        assert_eq!((u[0].cleanup_provider.as_deref(), u[0].tokens_in), (Some("openai"), 100));
        assert_eq!(db.delete_all_dictations().unwrap(), vec!["x.wav".to_string()]);
        let left = db.recent(10).unwrap();
        assert_eq!((left.len(), left[0].id), (1, live)); // the in-flight dictation survives
        assert!(db.usage_since(0).unwrap().is_empty(), "the estimate resets with History");
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn local_speeds_average_per_model() {
        let path = std::env::temp_dir().join(format!("yf-speed-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Storage::open(&path).unwrap();
        let d = |ms| NewDictation {
            app_exe: "",
            window_title: "",
            mode: "dictate",
            duration_ms: ms,
            audio_path: "x.wav",
            stt_provider: "whisper_cpp",
            stt_model: "w",
        };
        for (audio, stt, clean) in [(10_000, 1000, 800), (30_000, 2000, 0)] {
            let id = db.insert_dictation(&d(audio)).unwrap();
            db.finish_dictation(id, "a", "A.", &Timings { stt_ms: stt, cleanup_ms: clean, ..Default::default() }, "done").unwrap();
            db.set_cleanup_usage(id, "llama_cpp", "q", (0, 0)).unwrap();
        }
        let id = db.insert_dictation(&NewDictation { stt_provider: "openai", ..d(10_000) }).unwrap(); // cloud: ignored
        db.finish_dictation(id, "a", "A.", &Timings { stt_ms: 9000, ..Default::default() }, "done").unwrap();
        let mut s = db.local_speeds(0).unwrap();
        s.sort();
        // speech: 3000 ms over 40 s of audio = 750 per 10 s; cleanup: the skipped one doesn't count
        assert_eq!(s, vec![("q".to_string(), 1, 800), ("w".to_string(), 2, 750)]);
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
