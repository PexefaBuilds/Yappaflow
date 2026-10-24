//! Versioned schema via PRAGMA user_version. Append new steps; never edit old ones.

use anyhow::Result;
use rusqlite::Connection;

const STEPS: &[&str] = &[
    // v1 — Phase 1
    "CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
     );
     CREATE TABLE dictations (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        created_at   INTEGER NOT NULL,
        app_exe      TEXT NOT NULL DEFAULT '',
        window_title TEXT NOT NULL DEFAULT '',
        mode         TEXT NOT NULL DEFAULT 'hold',
        duration_ms  INTEGER NOT NULL DEFAULT 0,
        audio_path   TEXT,
        raw_text     TEXT,
        final_text   TEXT,
        stt_model    TEXT NOT NULL DEFAULT '',
        timings      TEXT,
        status       TEXT NOT NULL,
        error        TEXT
     );
     CREATE INDEX idx_dictations_created ON dictations(created_at);",
    // v2 — Phase 2: app styles, in priority order (first match wins)
    "CREATE TABLE app_styles (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        match_kind TEXT NOT NULL CHECK (match_kind IN ('exe', 'title_contains')),
        pattern    TEXT NOT NULL,
        style      TEXT NOT NULL
     );
     INSERT INTO app_styles(match_kind, pattern, style) VALUES
        ('exe', 'slack', 'casual'), ('exe', 'whatsapp', 'casual'),
        ('exe', 'discord', 'casual'), ('exe', 'telegram', 'casual'),
        ('exe', 'outlook', 'formal'), ('exe', 'olk', 'formal'),
        ('title_contains', 'Gmail', 'formal'), ('exe', 'winword', 'formal'),
        ('exe', 'code', 'literal'), ('exe', 'cursor', 'literal'),
        ('exe', 'windowsterminal', 'literal'), ('exe', 'wt', 'literal'),
        ('exe', 'powershell', 'literal'), ('exe', 'cmd', 'literal');",
    // v3 — Phase 4: dictionary (STT/cleanup hints) and snippets (spoken trigger → exact text)
    "CREATE TABLE dictionary (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        term        TEXT NOT NULL UNIQUE COLLATE NOCASE,
        sounds_like TEXT NOT NULL DEFAULT '',
        source      TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user', 'learned')),
        uses        INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL
     );
     CREATE TABLE snippets (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        \"trigger\" TEXT NOT NULL,
        expansion TEXT NOT NULL,
        uses      INTEGER NOT NULL DEFAULT 0
     );",
    // v4 — Phase 8: macOS rules match the bundle ID. SQLite can't alter a CHECK, so rebuild the table
    // (ids kept, so priority order is unchanged) and seed the Mac defaults after the Windows ones.
    "CREATE TABLE app_styles_v4 (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        match_kind TEXT NOT NULL CHECK (match_kind IN ('exe', 'title_contains', 'bundle_id')),
        pattern    TEXT NOT NULL,
        style      TEXT NOT NULL
     );
     INSERT INTO app_styles_v4(id, match_kind, pattern, style) SELECT id, match_kind, pattern, style FROM app_styles;
     DROP TABLE app_styles;
     ALTER TABLE app_styles_v4 RENAME TO app_styles;
     INSERT INTO app_styles(match_kind, pattern, style) VALUES
        ('bundle_id', 'com.tinyspeck.slackmacgap', 'casual'), ('bundle_id', 'com.hnc.Discord', 'casual'),
        ('bundle_id', 'ru.keepcoder.Telegram', 'casual'), ('bundle_id', 'net.whatsapp.WhatsApp', 'casual'),
        ('bundle_id', 'com.apple.MobileSMS', 'casual'),
        ('bundle_id', 'com.microsoft.Outlook', 'formal'), ('bundle_id', 'com.apple.mail', 'formal'),
        ('bundle_id', 'com.microsoft.Word', 'formal'),
        ('bundle_id', 'com.microsoft.VSCode', 'literal'), ('bundle_id', 'com.apple.Terminal', 'literal'),
        ('bundle_id', 'com.googlecode.iterm2', 'literal'), ('bundle_id', 'dev.warp.Warp-Stable', 'literal'),
        ('bundle_id', 'com.mitchellh.ghostty', 'literal'), ('bundle_id', 'com.todesktop.230313mzl4w4u92', 'literal');",
    // v5 — usage estimate. NULL providers mark rows from before v5 (cleanup cost unknown).
    "ALTER TABLE dictations ADD COLUMN stt_provider TEXT;
     ALTER TABLE dictations ADD COLUMN cleanup_provider TEXT;
     ALTER TABLE dictations ADD COLUMN cleanup_model TEXT NOT NULL DEFAULT '';
     ALTER TABLE dictations ADD COLUMN tokens_in INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE dictations ADD COLUMN tokens_out INTEGER NOT NULL DEFAULT 0;",
];

pub fn run(conn: &Connection) -> Result<()> {
    let current: usize = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    for (i, sql) in STEPS.iter().enumerate().skip(current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
        log::info!("db migrated to v{}", i + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn v4_keeps_rules_and_adds_bundle_ids() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        super::run(&c).unwrap();
        let n = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(n("SELECT COUNT(*) FROM app_styles WHERE match_kind != 'bundle_id'"), 14);
        assert_eq!(n("SELECT COUNT(*) FROM app_styles WHERE match_kind = 'bundle_id'"), 14);
        assert_eq!(n("SELECT MIN(id) FROM app_styles WHERE match_kind = 'bundle_id'"), 15, "Windows rows keep ids 1..14");
        c.execute("INSERT INTO app_styles(match_kind, pattern, style) VALUES ('bundle_id', 'x', 'casual')", []).unwrap();
        assert!(c.execute("INSERT INTO app_styles(match_kind, pattern, style) VALUES ('nope', 'x', 'casual')", []).is_err());
    }

    #[test]
    fn v5_keeps_old_rows_with_unknown_providers() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.pragma_update(None, "user_version", 0).unwrap();
        for sql in &super::STEPS[..4] {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 4).unwrap();
        c.execute("INSERT INTO dictations(created_at, status, stt_model) VALUES (1, 'done', 'nova-3')", []).unwrap();
        super::run(&c).unwrap();
        let (p, t): (Option<String>, i64) =
            c.query_row("SELECT cleanup_provider, tokens_in FROM dictations", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((p, t), (None, 0));
    }
}
