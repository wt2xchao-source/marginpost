use super::ports::{
    CoreResult, DocumentVersion, DocumentVersionSummary, PendingChangeSet, PendingRestore,
    VersionInput, VersionStore,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 2;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeSetMetadata {
    change_set_id: String,
    base_version_id: Option<String>,
    status: Option<String>,
    #[serde(default)]
    superseded_change_set_ids: Vec<String>,
}

pub struct SqliteVersionStore {
    connection: Mutex<Connection>,
}

impl SqliteVersionStore {
    pub fn open(path: &Path) -> CoreResult<Self> {
        Ok(Self {
            connection: Mutex::new(Connection::open(path)?),
        })
    }

    pub fn in_memory() -> CoreResult<Self> {
        Ok(Self {
            connection: Mutex::new(Connection::open_in_memory()?),
        })
    }

    fn ensure_column(
        connection: &Connection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> CoreResult<()> {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if !columns.iter().any(|name| name == column) {
            connection.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
        Ok(())
    }

    fn workspace_id(root_path: &str) -> String {
        stable_id("workspace", root_path)
    }

    fn document_id(root_path: &str, relative_path: &str) -> String {
        stable_id("document", &format!("{root_path}\n{relative_path}"))
    }

    fn ensure_document(
        connection: &Connection,
        root_path: &str,
        relative_path: &str,
        timestamp: u64,
    ) -> CoreResult<String> {
        let workspace_id = connection
            .query_row(
                "SELECT id FROM workspaces WHERE root_path = ?1",
                params![root_path],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| Self::workspace_id(root_path));
        connection.execute(
            "
            INSERT INTO workspaces (id, root_path, created_at, last_opened_at)
            VALUES (?1, ?2, ?3, ?3)
            ON CONFLICT(id) DO UPDATE SET last_opened_at = excluded.last_opened_at
            ",
            params![workspace_id, root_path, timestamp as i64],
        )?;
        let document_id = connection
            .query_row(
                "
                SELECT id FROM documents
                WHERE workspace_id = ?1 AND relative_path = ?2
                ",
                params![workspace_id, relative_path],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| Self::document_id(root_path, relative_path));
        connection.execute(
            "
            INSERT INTO documents (
                id, workspace_id, relative_path, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?4)
            ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at
            ",
            params![document_id, workspace_id, relative_path, timestamp as i64],
        )?;
        Ok(document_id)
    }

    fn version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentVersion> {
        Ok(DocumentVersion {
            id: row.get(0)?,
            relative_path: row.get(1)?,
            content: row.get(2)?,
            content_hash: row.get(3)?,
            encoding: row.get(4)?,
            version_type: row.get(5)?,
            created_at: row.get::<_, i64>(6)? as u64,
            source_type: row.get(7)?,
            source: row.get(8)?,
            agent: row.get(9)?,
            reason: row.get(10)?,
            schema_version: row.get(11)?,
            metadata_json: row.get(12)?,
        })
    }

    #[cfg(test)]
    fn count(&self, table: &str) -> CoreResult<i64> {
        let connection = self.connection.lock().expect("sqlite lock");
        let query = format!("SELECT COUNT(*) FROM {table}");
        Ok(connection.query_row(&query, [], |row| row.get(0))?)
    }
}

impl VersionStore for SqliteVersionStore {
    fn initialize(&self) -> CoreResult<()> {
        let connection = self.connection.lock().expect("sqlite lock");
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                root_path TEXT NOT NULL,
                schema_version INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                schema_version INTEGER NOT NULL DEFAULT 1,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(id)
            );
            ",
        )?;
        Self::ensure_column(
            &connection,
            "workspaces",
            "created_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(
            &connection,
            "workspaces",
            "last_opened_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(
            &connection,
            "documents",
            "created_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(
            &connection,
            "documents",
            "updated_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        connection.execute_batch(
            "
            CREATE UNIQUE INDEX IF NOT EXISTS idx_workspaces_root
                ON workspaces(root_path);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_workspace_path
                ON documents(workspace_id, relative_path);
            CREATE TABLE IF NOT EXISTS document_versions (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                parent_version_id TEXT,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                encoding TEXT NOT NULL,
                version_type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                source_type TEXT NOT NULL,
                source TEXT,
                agent TEXT,
                reason TEXT,
                schema_version INTEGER NOT NULL DEFAULT 2,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY(document_id) REFERENCES documents(id)
            );
            CREATE INDEX IF NOT EXISTS idx_versions_document_time
                ON document_versions(document_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS restore_operations (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                before_version_id TEXT NOT NULL,
                target_version_id TEXT NOT NULL,
                target_hash TEXT NOT NULL,
                restored_version_id TEXT,
                status TEXT NOT NULL,
                error_code TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER,
                schema_version INTEGER NOT NULL DEFAULT 2,
                FOREIGN KEY(document_id) REFERENCES documents(id)
            );
            PRAGMA user_version = 2;
            ",
        )?;
        Ok(())
    }

    fn create_workspace(&self, id: &str, root_path: &str) -> CoreResult<()> {
        let connection = self.connection.lock().expect("sqlite lock");
        let timestamp = now_millis();
        connection.execute(
            "
            INSERT INTO workspaces (id, root_path, created_at, last_opened_at)
            VALUES (?1, ?2, ?3, ?3)
            ",
            params![id, root_path, timestamp as i64],
        )?;
        Ok(())
    }

    fn create_document(&self, id: &str, workspace_id: &str, relative_path: &str) -> CoreResult<()> {
        let connection = self.connection.lock().expect("sqlite lock");
        let timestamp = now_millis();
        connection.execute(
            "
            INSERT INTO documents (id, workspace_id, relative_path, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            ",
            params![id, workspace_id, relative_path, timestamp as i64],
        )?;
        Ok(())
    }

    fn record_version(
        &self,
        input: &VersionInput,
        deduplicate_latest: bool,
    ) -> CoreResult<DocumentVersion> {
        let connection = self.connection.lock().expect("sqlite lock");
        let document_id = Self::ensure_document(
            &connection,
            &input.root_path,
            &input.relative_path,
            input.created_at,
        )?;
        if deduplicate_latest {
            let existing = connection
                .query_row(
                    "
                    SELECT v.id, d.relative_path, v.content, v.content_hash, v.encoding,
                           v.version_type, v.created_at, v.source_type, v.source, v.agent,
                           v.reason, v.schema_version, v.metadata_json
                    FROM document_versions v
                    JOIN documents d ON d.id = v.document_id
                    WHERE v.document_id = ?1
                    ORDER BY v.created_at DESC, v.rowid DESC
                    LIMIT 1
                    ",
                    params![document_id],
                    Self::version_from_row,
                )
                .optional()?;
            if let Some(version) = existing.filter(|version| {
                version.content_hash == input.content_hash && version.encoding == input.encoding
            }) {
                return Ok(version);
            }
        }
        let parent_version_id = connection
            .query_row(
                "
                SELECT id FROM document_versions
                WHERE document_id = ?1
                ORDER BY created_at DESC, rowid DESC
                LIMIT 1
                ",
                params![document_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let version_id = format!(
            "version-{}-{}",
            now_nanos(),
            &input.content_hash[..input.content_hash.len().min(12)]
        );
        connection.execute(
            "
            INSERT INTO document_versions (
                id, document_id, parent_version_id, content, content_hash, encoding,
                version_type, created_at, source_type, source, agent, reason,
                schema_version, metadata_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ",
            params![
                version_id,
                document_id,
                parent_version_id,
                input.content,
                input.content_hash,
                input.encoding,
                input.version_type,
                input.created_at as i64,
                input.source_type,
                input.source,
                input.agent,
                input.reason,
                SCHEMA_VERSION,
                input.metadata_json,
            ],
        )?;
        Ok(DocumentVersion {
            id: version_id,
            relative_path: input.relative_path.clone(),
            content: input.content.clone(),
            content_hash: input.content_hash.clone(),
            encoding: input.encoding.clone(),
            version_type: input.version_type.clone(),
            created_at: input.created_at,
            source_type: input.source_type.clone(),
            source: input.source.clone(),
            agent: input.agent.clone(),
            reason: input.reason.clone(),
            schema_version: SCHEMA_VERSION,
            metadata_json: input.metadata_json.clone(),
        })
    }

    fn list_versions(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> CoreResult<Vec<DocumentVersionSummary>> {
        let connection = self.connection.lock().expect("sqlite lock");
        let mut statement = connection.prepare(
            "
            SELECT v.id, d.relative_path, v.content_hash, v.encoding, v.version_type,
                   v.created_at, v.source_type, v.source, v.agent, v.reason,
                   v.schema_version
            FROM document_versions v
            JOIN documents d ON d.id = v.document_id
            JOIN workspaces w ON w.id = d.workspace_id
            WHERE w.root_path = ?1 AND d.relative_path = ?2
            ORDER BY v.created_at DESC, v.rowid DESC
            ",
        )?;
        let rows = statement
            .query_map(params![root_path, relative_path], |row| {
                Ok(DocumentVersionSummary {
                    id: row.get(0)?,
                    relative_path: row.get(1)?,
                    content_hash: row.get(2)?,
                    encoding: row.get(3)?,
                    version_type: row.get(4)?,
                    created_at: row.get::<_, i64>(5)? as u64,
                    source_type: row.get(6)?,
                    source: row.get(7)?,
                    agent: row.get(8)?,
                    reason: row.get(9)?,
                    schema_version: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn get_version(
        &self,
        root_path: &str,
        relative_path: &str,
        version_id: &str,
    ) -> CoreResult<Option<DocumentVersion>> {
        let connection = self.connection.lock().expect("sqlite lock");
        Ok(connection
            .query_row(
                "
                SELECT v.id, d.relative_path, v.content, v.content_hash, v.encoding,
                       v.version_type, v.created_at, v.source_type, v.source, v.agent,
                       v.reason, v.schema_version, v.metadata_json
                FROM document_versions v
                JOIN documents d ON d.id = v.document_id
                JOIN workspaces w ON w.id = d.workspace_id
                WHERE w.root_path = ?1 AND d.relative_path = ?2 AND v.id = ?3
                ",
                params![root_path, relative_path, version_id],
                Self::version_from_row,
            )
            .optional()?)
    }

    fn begin_restore(
        &self,
        root_path: &str,
        relative_path: &str,
        before_version_id: &str,
        target_version_id: &str,
        target_hash: &str,
        created_at: u64,
    ) -> CoreResult<String> {
        let connection = self.connection.lock().expect("sqlite lock");
        let document_id = Self::ensure_document(&connection, root_path, relative_path, created_at)?;
        let operation_id = format!("restore-{}", now_nanos());
        connection.execute(
            "
            INSERT INTO restore_operations (
                id, document_id, before_version_id, target_version_id, target_hash,
                status, created_at, schema_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)
            ",
            params![
                operation_id,
                document_id,
                before_version_id,
                target_version_id,
                target_hash,
                created_at as i64,
                SCHEMA_VERSION
            ],
        )?;
        Ok(operation_id)
    }

    fn complete_restore(
        &self,
        operation_id: &str,
        restored_version_id: &str,
        completed_at: u64,
    ) -> CoreResult<()> {
        let connection = self.connection.lock().expect("sqlite lock");
        connection.execute(
            "
            UPDATE restore_operations
            SET status = 'completed', restored_version_id = ?2, completed_at = ?3
            WHERE id = ?1 AND status = 'pending'
            ",
            params![operation_id, restored_version_id, completed_at as i64],
        )?;
        Ok(())
    }

    fn fail_restore(
        &self,
        operation_id: &str,
        error_code: &str,
        completed_at: u64,
    ) -> CoreResult<()> {
        let connection = self.connection.lock().expect("sqlite lock");
        connection.execute(
            "
            UPDATE restore_operations
            SET status = 'failed', error_code = ?2, completed_at = ?3
            WHERE id = ?1 AND status = 'pending'
            ",
            params![operation_id, error_code, completed_at as i64],
        )?;
        Ok(())
    }

    fn pending_restores(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> CoreResult<Vec<PendingRestore>> {
        let connection = self.connection.lock().expect("sqlite lock");
        let mut statement = connection.prepare(
            "
            SELECT r.id, w.root_path, d.relative_path, r.before_version_id,
                   r.target_version_id, r.target_hash
            FROM restore_operations r
            JOIN documents d ON d.id = r.document_id
            JOIN workspaces w ON w.id = d.workspace_id
            WHERE w.root_path = ?1 AND d.relative_path = ?2 AND r.status = 'pending'
            ORDER BY r.created_at
            ",
        )?;
        let rows = statement
            .query_map(params![root_path, relative_path], |row| {
                Ok(PendingRestore {
                    id: row.get(0)?,
                    root_path: row.get(1)?,
                    relative_path: row.get(2)?,
                    before_version_id: row.get(3)?,
                    target_version_id: row.get(4)?,
                    target_hash: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn pending_change_sets(&self, root_path: &str) -> CoreResult<Vec<PendingChangeSet>> {
        let connection = self.connection.lock().expect("sqlite lock");
        let mut statement = connection.prepare(
            "
            SELECT v.id, d.relative_path, v.content, v.content_hash, v.encoding,
                   v.created_at, v.metadata_json, v.source
            FROM document_versions v
            JOIN documents d ON d.id = v.document_id
            JOIN workspaces w ON w.id = d.workspace_id
            WHERE w.root_path = ?1 AND v.version_type = 'external'
            ORDER BY v.created_at, v.rowid
            ",
        )?;
        let candidates = statement
            .query_map(params![root_path], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut resolved_statement = connection.prepare(
            "
            SELECT v.metadata_json
            FROM document_versions v
            JOIN documents d ON d.id = v.document_id
            JOIN workspaces w ON w.id = d.workspace_id
            WHERE w.root_path = ?1 AND v.version_type LIKE 'review_%'
            ",
        )?;
        let resolved_metadata = resolved_statement
            .query_map(params![root_path], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|metadata| serde_json::from_str::<ChangeSetMetadata>(&metadata).ok())
            .collect::<Vec<_>>();
        let mut resolved_ids = std::collections::HashSet::new();
        for metadata in resolved_metadata {
            resolved_ids.insert(metadata.change_set_id);
            resolved_ids.extend(metadata.superseded_change_set_ids);
        }

        let mut pending = Vec::new();
        for (
            candidate_version_id,
            relative_path,
            candidate_content,
            candidate_hash,
            candidate_encoding,
            detected_at,
            metadata_json,
            source,
        ) in candidates
        {
            let Ok(metadata) = serde_json::from_str::<ChangeSetMetadata>(&metadata_json) else {
                continue;
            };
            if metadata.status.as_deref() != Some("pending")
                || resolved_ids.contains(&metadata.change_set_id)
            {
                continue;
            }
            let Some(base_version_id) = metadata.base_version_id else {
                continue;
            };
            let Some(base) = connection
                .query_row(
                    "
                    SELECT v.content, v.content_hash, v.encoding
                    FROM document_versions v
                    JOIN documents d ON d.id = v.document_id
                    JOIN workspaces w ON w.id = d.workspace_id
                    WHERE w.root_path = ?1 AND d.relative_path = ?2 AND v.id = ?3
                    ",
                    params![root_path, relative_path, base_version_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
            else {
                continue;
            };
            pending.push(PendingChangeSet {
                id: metadata.change_set_id,
                relative_path,
                base_version_id,
                base_content: base.0,
                base_hash: base.1,
                base_encoding: base.2,
                candidate_version_id,
                candidate_content,
                candidate_hash,
                candidate_encoding,
                source,
                detected_at,
            });
        }
        Ok(pending)
    }
}

fn stable_id(prefix: &str, value: &str) -> String {
    format!("{prefix}-{:x}", Sha256::digest(value.as_bytes()))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(content: &str, hash: &str, version_type: &str, created_at: u64) -> VersionInput {
        VersionInput {
            root_path: "/tmp/workspace".to_string(),
            relative_path: "article.md".to_string(),
            content: content.to_string(),
            content_hash: hash.to_string(),
            encoding: "utf-8".to_string(),
            version_type: version_type.to_string(),
            created_at,
            source_type: "editor".to_string(),
            source: None,
            agent: None,
            reason: None,
            metadata_json: "{}".to_string(),
        }
    }

    #[test]
    fn migrates_the_legacy_schema_and_persists_versions() {
        let store = SqliteVersionStore::in_memory().expect("open in-memory sqlite");
        {
            let connection = store.connection.lock().expect("sqlite lock");
            connection
                .execute_batch(
                    "
                    CREATE TABLE workspaces (
                        id TEXT PRIMARY KEY,
                        root_path TEXT NOT NULL,
                        schema_version INTEGER NOT NULL DEFAULT 1
                    );
                    CREATE TABLE documents (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        relative_path TEXT NOT NULL,
                        schema_version INTEGER NOT NULL DEFAULT 1
                    );
                    INSERT INTO workspaces (id, root_path)
                    VALUES ('legacy-workspace', '/tmp/workspace');
                    INSERT INTO documents (id, workspace_id, relative_path)
                    VALUES ('legacy-document', 'legacy-workspace', 'article.md');
                    ",
                )
                .expect("legacy schema");
        }
        store.initialize().expect("migrate");
        let version = store
            .record_version(&input("# One", "hash-one", "snapshot", 1), true)
            .expect("record");

        assert_eq!(store.count("document_versions").expect("count versions"), 1);
        assert_eq!(store.count("workspaces").expect("count workspaces"), 1);
        assert_eq!(store.count("documents").expect("count documents"), 1);
        assert_eq!(
            store
                .get_version("/tmp/workspace", "article.md", &version.id)
                .expect("get version")
                .expect("version")
                .content,
            "# One"
        );
    }

    #[test]
    fn deduplicates_snapshots_but_keeps_action_versions() {
        let store = SqliteVersionStore::in_memory().expect("open in-memory sqlite");
        store.initialize().expect("initialize");
        store
            .record_version(&input("# One", "hash-one", "snapshot", 1), true)
            .expect("first snapshot");
        store
            .record_version(&input("# One", "hash-one", "snapshot", 2), true)
            .expect("duplicate snapshot");
        store
            .record_version(&input("# One", "hash-one", "restore", 3), false)
            .expect("restore event");

        assert_eq!(store.count("document_versions").expect("count versions"), 2);
    }

    #[test]
    fn records_and_completes_restore_operations() {
        let store = SqliteVersionStore::in_memory().expect("open in-memory sqlite");
        store.initialize().expect("initialize");
        let before = store
            .record_version(&input("# Current", "current", "snapshot", 1), true)
            .expect("before");
        let target = store
            .record_version(&input("# Target", "target", "editor", 2), false)
            .expect("target");
        let operation = store
            .begin_restore(
                "/tmp/workspace",
                "article.md",
                &before.id,
                &target.id,
                &target.content_hash,
                3,
            )
            .expect("begin");

        assert_eq!(
            store
                .pending_restores("/tmp/workspace", "article.md")
                .expect("pending")
                .len(),
            1
        );
        store
            .complete_restore(&operation, &target.id, 4)
            .expect("complete");
        assert!(store
            .pending_restores("/tmp/workspace", "article.md")
            .expect("pending")
            .is_empty());
    }

    #[test]
    fn reopens_a_file_database_with_history_intact() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("history.sqlite3");
        let version_id = {
            let store = SqliteVersionStore::open(&path).expect("open store");
            store.initialize().expect("initialize");
            store
                .record_version(&input("# Persisted", "persisted", "editor", 1), false)
                .expect("record")
                .id
        };
        let reopened = SqliteVersionStore::open(&path).expect("reopen store");
        reopened.initialize().expect("reinitialize");

        let versions = reopened
            .list_versions("/tmp/workspace", "article.md")
            .expect("list");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, version_id);
    }
}
