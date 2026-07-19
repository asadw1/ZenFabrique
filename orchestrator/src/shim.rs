use crate::rdf::AliasMap;
use anyhow::{Context, Result};
use duckdb::{params, Connection};
use std::path::Path;

pub struct ShimEngine {
    conn: Connection,
    aliases: AliasMap,
    next_id: i64,
}

impl ShimEngine {
    pub fn open(path: &Path, initial_aliases: AliasMap) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open DuckDB database at {}", path.display()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS raw_events (
                id BIGINT PRIMARY KEY,
                source_path TEXT,
                payload JSON,
                received_at TIMESTAMP DEFAULT current_timestamp
            );
            CREATE TABLE IF NOT EXISTS shim_aliases (
                canonical TEXT,
                raw_key TEXT,
                learned_at TIMESTAMP DEFAULT current_timestamp,
                last_used_at TIMESTAMP DEFAULT current_timestamp,
                PRIMARY KEY (canonical, raw_key)
            );",
        )
        .context("failed to create raw_events/shim_aliases tables")?;

        let next_id: i64 = conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM raw_events", [], |r| {
                r.get(0)
            })
            .context("failed to determine next raw_events id")?;

        let mut aliases = initial_aliases;
        {
            let mut stmt = conn
                .prepare("SELECT canonical, raw_key FROM shim_aliases")
                .context("failed to prepare shim_aliases load query")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let canonical: String = row.get(0)?;
                let raw_key: String = row.get(1)?;
                let entry = aliases.entry(canonical).or_default();
                if !entry.iter().any(|k| k == &raw_key) {
                    entry.push(raw_key);
                }
            }
        }

        let engine = Self {
            conn,
            aliases,
            next_id,
        };
        engine.regenerate_view()?;
        Ok(engine)
    }

    pub fn insert_raw(&mut self, source_path: &str, payload_text: &str) -> Result<i64> {
        self.next_id += 1;
        let id = self.next_id;
        self.conn
            .execute(
                "INSERT INTO raw_events (id, source_path, payload) VALUES (?, ?, ?)",
                params![id, source_path, payload_text],
            )
            .context("failed to insert raw event")?;
        Ok(id)
    }

    pub fn aliases(&self) -> &AliasMap {
        &self.aliases
    }

    // Widens the alias registry with a newly-discovered renamed field and
    // persists it to `shim_aliases` so the repair survives a restart — a
    // self-healing fix that's forgotten on the next run isn't actually
    // healed. The caller regenerates the view afterward.
    pub fn learn_alias(&mut self, canonical: &str, raw_key: &str) -> Result<bool> {
        let entry = self.aliases.entry(canonical.to_string()).or_default();
        if entry.iter().any(|k| k == raw_key) {
            return Ok(false);
        }
        entry.push(raw_key.to_string());

        self.conn
            .execute(
                "INSERT OR IGNORE INTO shim_aliases (canonical, raw_key) VALUES (?, ?)",
                params![canonical, raw_key],
            )
            .context("failed to persist learned alias")?;

        Ok(true)
    }

    // Records that a learned alias was actually exercised just now. There's
    // no automatic expiry — deciding a rename is "obsolete" needs a human,
    // not a heuristic — but `last_used_at` at least gives an operator
    // something to audit before trusting an old alias (E1: a key can be
    // silently repurposed for something else long after it was learned).
    pub fn touch_alias(&mut self, canonical: &str, raw_key: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE shim_aliases SET last_used_at = current_timestamp
                 WHERE canonical = ? AND raw_key = ?",
                params![canonical, raw_key],
            )
            .context("failed to update alias last_used_at")?;
        Ok(())
    }

    // Rebuilds the `streaming_events` shim view from the current alias
    // registry. Because it's a VIEW over `raw_events` rather than a
    // materialized copy, widening the aliases retroactively heals every
    // past row that used the newly-recognized key, not just future ones.
    pub fn regenerate_view(&self) -> Result<()> {
        let sql = build_view_sql(&self.aliases);
        self.conn
            .execute_batch(&sql)
            .context("failed to regenerate streaming_events shim view")?;
        Ok(())
    }

    // Read-only introspection used by tests and available for manual audit
    // (e.g. spotting an alias whose `last_used_at` hasn't moved in a while).
    // Not yet called from production code, hence the `allow`.
    #[allow(dead_code)]
    pub fn alias_audit(&self) -> Result<Vec<AliasAuditRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical, raw_key, CAST(learned_at AS VARCHAR), CAST(last_used_at AS VARCHAR)
             FROM shim_aliases ORDER BY canonical, raw_key",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(AliasAuditRow {
                canonical: row.get(0)?,
                raw_key: row.get(1)?,
                learned_at: row.get(2)?,
                last_used_at: row.get(3)?,
            });
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn query_streaming_event(&self, event_id: &str) -> Result<Option<StreamingEventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT user_id, track_id, event_timestamp, ms_played
             FROM streaming_events WHERE event_id = ?",
        )?;
        let mut rows = stmt.query(params![event_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(StreamingEventRow {
                user_id: row.get(0)?,
                track_id: row.get(1)?,
                event_timestamp: row.get(2)?,
                ms_played: row.get(3)?,
            })),
            None => Ok(None),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct AliasAuditRow {
    pub canonical: String,
    pub raw_key: String,
    pub learned_at: String,
    pub last_used_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingEventRow {
    pub user_id: Option<String>,
    pub track_id: Option<String>,
    pub event_timestamp: Option<String>,
    pub ms_played: Option<i64>,
}

fn build_view_sql(aliases: &AliasMap) -> String {
    format!(
        "CREATE OR REPLACE VIEW streaming_events AS
         SELECT
           id,
           source_path,
           received_at,
           {event_id} AS event_id,
           {timestamp} AS event_timestamp,
           {ms_played} AS ms_played,
           {user_id} AS user_id,
           {track_id} AS track_id
         FROM raw_events;",
        event_id = coalesce_for(aliases, "eventId", false),
        timestamp = coalesce_for(aliases, "timestamp", false),
        ms_played = coalesce_for(aliases, "msPlayed", true),
        user_id = coalesce_for(aliases, "userId", false),
        track_id = coalesce_for(aliases, "trackId", false),
    )
}

fn coalesce_for(aliases: &AliasMap, field: &str, as_bigint: bool) -> String {
    let exprs: Vec<String> = aliases
        .get(field)
        .map(|keys| keys.iter().filter(|k| is_safe_json_key(k)).collect::<Vec<_>>())
        .unwrap_or_default()
        .iter()
        .map(|k| {
            if as_bigint {
                format!("TRY_CAST(json_extract_string(payload, '$.{k}') AS BIGINT)")
            } else {
                format!("json_extract_string(payload, '$.{k}')")
            }
        })
        .collect();

    if exprs.is_empty() {
        "NULL".to_string()
    } else {
        format!("COALESCE({})", exprs.join(", "))
    }
}

// Guards against interpolating anything unsafe into the generated SQL's
// JSONPath literal — canonical field names and learned aliases are the only
// inputs, but both ultimately trace back to external JSON keys.
fn is_safe_json_key(k: &str) -> bool {
    let mut chars = k.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// E1/E2 coverage (docs/testing/self-healing-adversarial-matrix.md): alias
// persistence across restarts, idempotency under repeat exposure, and the
// audit-trail columns that give an operator something to check before
// trusting an old alias.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf;
    use std::path::PathBuf;

    fn temp_db_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("zenfabrique_test_{name}_{nanos}.duckdb"))
    }

    // Baseline: a healed event's canonical fields show up correctly in the
    // streaming_events view — the same thing verified live in Phase 3, now
    // as a repeatable assertion.
    #[test]
    fn baseline_learned_alias_heals_the_view() {
        let path = temp_db_path("baseline");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();

        shim.learn_alias("userId", "user_id").unwrap();
        shim.regenerate_view().unwrap();
        shim.insert_raw(
            "test.json",
            r#"{"eventId":"e1","user_id":"u1","trackId":"t1","timestamp":"2026-01-01T00:00:00","msPlayed":1000}"#,
        )
        .unwrap();

        let row = shim.query_streaming_event("e1").unwrap().unwrap();
        assert_eq!(row.user_id, Some("u1".to_string()));
        assert_eq!(row.track_id, Some("t1".to_string()));
        assert_eq!(row.ms_played, Some(1000));

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    // E2 — idempotency: learning the same (canonical, raw_key) pair twice
    // must not error, must not duplicate the shim_aliases row, and the
    // second call should report "nothing new" via its return value.
    #[test]
    fn e2_learning_same_alias_twice_is_idempotent() {
        let path = temp_db_path("e2_idempotent");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();

        let first = shim.learn_alias("userId", "user_id").unwrap();
        let second = shim.learn_alias("userId", "user_id").unwrap();
        assert!(first, "first learn_alias call should report a new alias");
        assert!(!second, "repeat learn_alias call should report nothing new");

        let audit = shim.alias_audit().unwrap();
        assert_eq!(audit.len(), 1, "expected exactly one shim_aliases row, got {audit:?}");
        assert_eq!(audit[0].canonical, "userId");
        assert_eq!(audit[0].raw_key, "user_id");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }

    // E1 — restart durability: reopening the same database file must load
    // the previously-learned alias without needing to relearn it, and the
    // resulting view must already reflect the fix on the very first query
    // (mirrors the live restart test done manually during Phase 3).
    #[test]
    fn e1_alias_survives_reopen() {
        let path = temp_db_path("e1_restart");
        {
            let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
            shim.learn_alias("userId", "user_id").unwrap();
        } // simulates process exit — connection dropped

        {
            let shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();
            assert!(shim.aliases().get("userId").unwrap().contains(&"user_id".to_string()));

            let audit = shim.alias_audit().unwrap();
            assert_eq!(audit.len(), 1);
            assert_eq!(audit[0].raw_key, "user_id");
        }

        let _ = std::fs::remove_file(&path);
    }

    // E1 — audit trail: touch_alias must update last_used_at for a real
    // alias, and must be a safe no-op (no error, no phantom row) for a key
    // that was never actually learned as an alias.
    #[test]
    fn e1_touch_alias_updates_audit_and_ignores_unknown_pairs() {
        let path = temp_db_path("e1_touch");
        let mut shim = ShimEngine::open(&path, rdf::default_aliases()).unwrap();

        shim.learn_alias("userId", "user_id").unwrap();
        shim.touch_alias("userId", "user_id").unwrap();
        let audit = shim.alias_audit().unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].canonical, "userId");
        assert_eq!(audit[0].raw_key, "user_id");
        assert!(!audit[0].learned_at.is_empty());
        assert!(!audit[0].last_used_at.is_empty());

        // touching a pair that was never learned (e.g. the bare canonical
        // name, which never gets a shim_aliases row) must not error or
        // create a phantom entry.
        shim.touch_alias("userId", "userId").unwrap();
        let audit_after = shim.alias_audit().unwrap();
        assert_eq!(audit_after.len(), 1, "touching an unknown pair must not create a row");

        drop(shim);
        let _ = std::fs::remove_file(&path);
    }
}
