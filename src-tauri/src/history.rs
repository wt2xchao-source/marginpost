use crate::change_monitor::{ChangeMonitorState, ChangeSetDetail};
use crate::commands::{
    content_hash, encode_markdown, read_markdown_file_sync, save_markdown_file_sync, CommandError,
    OpenedDocument,
};
use crate::core::{
    DiffChange, DiffEngine, DocumentVersion, DocumentVersionSummary, ParagraphDiffEngine,
    SqliteVersionStore, VersionInput, VersionStore,
};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

pub struct HistoryState {
    store: SqliteVersionStore,
}

impl HistoryState {
    pub fn new(store: SqliteVersionStore) -> Self {
        Self { store }
    }

    pub(crate) fn capture_snapshot(
        &self,
        root_path: &str,
        document: &OpenedDocument,
        version_type: &str,
        source_type: &str,
    ) -> Result<DocumentVersion, CommandError> {
        self.record(
            root_path,
            &document.relative_path,
            &document.content,
            &document.content_hash,
            &document.encoding,
            version_type,
            source_type,
            None,
            "{}",
            true,
        )
    }

    pub(crate) fn capture_action(
        &self,
        root_path: &str,
        relative_path: &str,
        content: &str,
        content_hash: &str,
        encoding: &str,
        version_type: &str,
        source_type: &str,
        source: Option<String>,
        metadata_json: &str,
    ) -> Result<DocumentVersion, CommandError> {
        self.record(
            root_path,
            relative_path,
            content,
            content_hash,
            encoding,
            version_type,
            source_type,
            source,
            metadata_json,
            false,
        )
    }

    fn record(
        &self,
        root_path: &str,
        relative_path: &str,
        content: &str,
        content_hash: &str,
        encoding: &str,
        version_type: &str,
        source_type: &str,
        source: Option<String>,
        metadata_json: &str,
        deduplicate_latest: bool,
    ) -> Result<DocumentVersion, CommandError> {
        self.store
            .record_version(
                &VersionInput {
                    root_path: root_path.to_string(),
                    relative_path: relative_path.to_string(),
                    content: content.to_string(),
                    content_hash: content_hash.to_string(),
                    encoding: encoding.to_string(),
                    version_type: version_type.to_string(),
                    created_at: now_millis(),
                    source_type: source_type.to_string(),
                    source,
                    agent: None,
                    reason: None,
                    metadata_json: metadata_json.to_string(),
                },
                deduplicate_latest,
            )
            .map_err(history_error)
    }

    fn reconcile_pending(&self, root_path: &str, relative_path: &str) -> Result<(), CommandError> {
        let pending = self
            .store
            .pending_restores(root_path, relative_path)
            .map_err(history_error)?;
        if pending.is_empty() {
            return Ok(());
        }
        let opened = read_markdown_file_sync(root_path, relative_path)?;
        for operation in pending {
            let target = self
                .store
                .get_version(root_path, relative_path, &operation.target_version_id)
                .map_err(history_error)?;
            let before = self
                .store
                .get_version(root_path, relative_path, &operation.before_version_id)
                .map_err(history_error)?;
            if opened.content_hash == operation.target_hash {
                if let Some(target) = target {
                    let restored = self.capture_action(
                        &operation.root_path,
                        &operation.relative_path,
                        &target.content,
                        &target.content_hash,
                        &target.encoding,
                        "restore",
                        "history",
                        None,
                        &format!(r#"{{"targetVersionId":"{}","recovered":true}}"#, target.id),
                    )?;
                    self.store
                        .complete_restore(&operation.id, &restored.id, now_millis())
                        .map_err(history_error)?;
                } else {
                    self.store
                        .fail_restore(&operation.id, "TARGET_VERSION_MISSING", now_millis())
                        .map_err(history_error)?;
                }
            } else if before
                .as_ref()
                .is_some_and(|version| version.content_hash == opened.content_hash)
            {
                self.store
                    .fail_restore(&operation.id, "RESTORE_INTERRUPTED", now_millis())
                    .map_err(history_error)?;
            } else {
                self.store
                    .fail_restore(&operation.id, "RESTORE_CONFLICT", now_millis())
                    .map_err(history_error)?;
            }
        }
        Ok(())
    }

    pub(crate) fn persist_change_set(
        &self,
        root_path: &str,
        detail: &ChangeSetDetail,
    ) -> Result<(String, String), CommandError> {
        let base_relative_path = detail
            .summary
            .previous_relative_path
            .as_deref()
            .unwrap_or(&detail.summary.relative_path);
        let existing_base = self
            .store
            .list_versions(root_path, base_relative_path)
            .map_err(history_error)?
            .into_iter()
            .find(|version| {
                version.content_hash == detail.summary.base_hash
                    && version.encoding == detail.base_encoding
            });
        let base = if let Some(version) = existing_base {
            self.store
                .get_version(root_path, base_relative_path, &version.id)
                .map_err(history_error)?
                .ok_or_else(|| {
                    CommandError::new(
                        "HISTORY_STORE_FAILED",
                        "The matching base version could not be loaded.",
                    )
                })?
        } else {
            self.record(
                root_path,
                base_relative_path,
                &detail.base_content,
                &detail.summary.base_hash,
                &detail.base_encoding,
                "snapshot",
                "filesystem",
                None,
                "{}",
                true,
            )?
        };
        let candidate = self.capture_action(
            root_path,
            &detail.summary.relative_path,
            &detail.candidate_content,
            &detail.summary.candidate_hash,
            &detail.candidate_encoding,
            "external",
            "external",
            detail.summary.source.clone(),
            &serde_json::json!({
                "changeSetId": detail.summary.id,
                "baseVersionId": base.id,
                "status": detail.summary.status,
                "changeType": detail.summary.change_type,
                "previousRelativePath": detail.summary.previous_relative_path,
                "baseRelativePath": base_relative_path,
                "baseExists": detail.base_exists,
                "candidateExists": detail.candidate_exists,
                "supersededBy": detail.summary.superseded_by,
                "supersededChangeSetIds": detail.summary.superseded_change_set_ids
            })
            .to_string(),
        )?;
        Ok((base.id, candidate.id))
    }

    pub(crate) fn pending_change_sets(
        &self,
        root_path: &str,
    ) -> Result<Vec<crate::core::PendingChangeSet>, CommandError> {
        self.store
            .pending_change_sets(root_path)
            .map_err(history_error)
    }

    pub(crate) fn mark_change_set_stale(
        &self,
        root_path: &str,
        detail: &ChangeSetDetail,
    ) -> Result<(), CommandError> {
        self.capture_action(
            root_path,
            &detail.summary.relative_path,
            &detail.candidate_content,
            &detail.summary.candidate_hash,
            &detail.candidate_encoding,
            "external",
            "external",
            detail.summary.source.clone(),
            &serde_json::json!({
                "changeSetId": detail.summary.id,
                "baseVersionId": detail.summary.base_version_id,
                "status": "stale",
                "changeType": detail.summary.change_type,
                "previousRelativePath": detail.summary.previous_relative_path,
                "baseRelativePath": detail.summary.previous_relative_path.as_deref()
                    .unwrap_or(&detail.summary.relative_path),
                "baseExists": detail.base_exists,
                "candidateExists": detail.candidate_exists,
                "supersededBy": detail.summary.superseded_by,
                "supersededChangeSetIds": detail.summary.superseded_change_set_ids
            })
            .to_string(),
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    content: String,
    content_hash: String,
    encoding: String,
    version: DocumentVersionSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VersionComparison {
    base: DocumentVersionSummary,
    candidate: DocumentVersionSummary,
    changes: Vec<DiffChange>,
}

impl From<DocumentVersion> for DocumentVersionSummary {
    fn from(version: DocumentVersion) -> Self {
        Self {
            id: version.id,
            relative_path: version.relative_path,
            content_hash: version.content_hash,
            encoding: version.encoding,
            version_type: version.version_type,
            created_at: version.created_at,
            source_type: version.source_type,
            source: version.source,
            agent: version.agent,
            reason: version.reason,
            schema_version: version.schema_version,
        }
    }
}

#[tauri::command]
pub async fn list_document_versions(
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
) -> Result<Vec<DocumentVersionSummary>, CommandError> {
    history.reconcile_pending(&root_path, &relative_path)?;
    history
        .store
        .list_versions(&root_path, &relative_path)
        .map_err(history_error)
}

#[tauri::command]
pub fn get_document_version(
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
    version_id: String,
) -> Result<Option<DocumentVersion>, CommandError> {
    history
        .store
        .get_version(&root_path, &relative_path, &version_id)
        .map_err(history_error)
}

#[tauri::command]
pub fn compare_document_versions(
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
    base_version_id: String,
    candidate_version_id: String,
) -> Result<VersionComparison, CommandError> {
    compare_document_versions_sync(
        &history,
        &root_path,
        &relative_path,
        &base_version_id,
        &candidate_version_id,
    )
}

fn compare_document_versions_sync(
    history: &HistoryState,
    root_path: &str,
    relative_path: &str,
    base_version_id: &str,
    candidate_version_id: &str,
) -> Result<VersionComparison, CommandError> {
    let load = |version_id: &str| {
        history
            .store
            .get_version(root_path, relative_path, version_id)
            .map_err(history_error)?
            .ok_or_else(|| {
                CommandError::new(
                    "VERSION_UNAVAILABLE",
                    "This historical version is no longer available.",
                )
            })
    };
    let base = load(base_version_id)?;
    let candidate = load(candidate_version_id)?;
    let changes = ParagraphDiffEngine.compare(&base.content, &candidate.content);
    Ok(VersionComparison {
        base: base.into(),
        candidate: candidate.into(),
        changes,
    })
}

#[tauri::command]
pub fn restore_document_version(
    monitor: State<'_, ChangeMonitorState>,
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
    version_id: String,
) -> Result<RestoreResult, CommandError> {
    restore_document_version_sync(&monitor, &history, &root_path, &relative_path, &version_id)
}

fn restore_document_version_sync(
    monitor: &ChangeMonitorState,
    history: &HistoryState,
    root_path: &str,
    relative_path: &str,
    version_id: &str,
) -> Result<RestoreResult, CommandError> {
    history.reconcile_pending(&root_path, &relative_path)?;
    let target = history
        .store
        .get_version(&root_path, &relative_path, &version_id)
        .map_err(history_error)?
        .ok_or_else(|| {
            CommandError::new(
                "VERSION_UNAVAILABLE",
                "This historical version is no longer available.",
            )
        })?;
    let opened = read_markdown_file_sync(root_path, relative_path)?;
    let before = history.capture_snapshot(&root_path, &opened, "pre_restore", "history")?;
    let operation_id = history
        .store
        .begin_restore(
            &root_path,
            &relative_path,
            &before.id,
            &target.id,
            &target.content_hash,
            now_millis(),
        )
        .map_err(history_error)?;
    let planned_bytes = encode_markdown(&target.content, &target.encoding)?;
    let planned_hash = content_hash(&planned_bytes);
    monitor.prepare_internal_save(&relative_path, &planned_hash);
    let save_result = save_markdown_file_sync(
        root_path,
        relative_path,
        &target.content,
        &opened.content_hash,
        &target.encoding,
    );

    let saved = match save_result {
        Ok(saved) => saved,
        Err(error) => {
            monitor.cancel_internal_save(&relative_path, &planned_hash);
            let _ = history
                .store
                .fail_restore(&operation_id, &error.code, now_millis());
            return Err(error);
        }
    };
    monitor.confirm_internal_save(&relative_path, &target.content, &saved.content_hash);
    let restored = history.capture_action(
        &root_path,
        &relative_path,
        &target.content,
        &saved.content_hash,
        &target.encoding,
        "restore",
        "history",
        None,
        &format!(r#"{{"targetVersionId":"{}"}}"#, target.id),
    )?;
    history
        .store
        .complete_restore(&operation_id, &restored.id, now_millis())
        .map_err(history_error)?;
    Ok(RestoreResult {
        content: target.content,
        content_hash: saved.content_hash,
        encoding: target.encoding,
        version: restored.into(),
    })
}

fn history_error(error: Box<dyn std::error::Error + Send + Sync>) -> CommandError {
    CommandError::new("HISTORY_STORE_FAILED", error.to_string())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::read_markdown_file_sync;
    use crate::core::VersionStore;
    use std::fs;

    fn state() -> HistoryState {
        let store = SqliteVersionStore::in_memory().expect("store");
        store.initialize().expect("initialize");
        HistoryState::new(store)
    }

    #[test]
    fn captures_and_lists_document_versions() {
        let history = state();
        let document = OpenedDocument {
            relative_path: "note.md".to_string(),
            content: "# One".to_string(),
            content_hash: "hash-one".to_string(),
            encoding: "utf-8".to_string(),
        };
        history
            .capture_snapshot("/workspace", &document, "snapshot", "filesystem")
            .expect("capture");
        history
            .capture_action(
                "/workspace",
                "note.md",
                "# Two",
                "hash-two",
                "utf-8",
                "editor",
                "editor",
                None,
                "{}",
            )
            .expect("capture action");

        let versions = history
            .store
            .list_versions("/workspace", "note.md")
            .expect("list");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version_type, "editor");
    }

    #[test]
    fn compares_two_versions_with_structured_markdown_changes() {
        let history = state();
        let base = history
            .capture_action(
                "/workspace",
                "note.md",
                "# Title\n\nOld paragraph.",
                "hash-base",
                "utf-8",
                "editor",
                "editor",
                None,
                "{}",
            )
            .expect("base");
        let candidate = history
            .capture_action(
                "/workspace",
                "note.md",
                "# Better title\n\nNew paragraph.",
                "hash-candidate",
                "utf-8",
                "editor",
                "editor",
                None,
                "{}",
            )
            .expect("candidate");

        let comparison = compare_document_versions_sync(
            &history,
            "/workspace",
            "note.md",
            &base.id,
            &candidate.id,
        )
        .expect("compare");

        assert_eq!(comparison.base.id, base.id);
        assert_eq!(comparison.candidate.id, candidate.id);
        assert_eq!(comparison.changes.len(), 2);
    }

    #[test]
    fn interrupted_restore_is_marked_failed_when_disk_keeps_before_version() {
        let directory = tempfile::tempdir().expect("temp directory");
        fs::write(directory.path().join("note.md"), "# Current").expect("write");
        let root = directory.path().to_string_lossy().into_owned();
        let history = state();
        let opened = read_markdown_file_sync(&root, "note.md").expect("open");
        let before = history
            .capture_snapshot(&root, &opened, "snapshot", "filesystem")
            .expect("before");
        let target = history
            .capture_action(
                &root,
                "note.md",
                "# Target",
                &content_hash(b"# Target"),
                "utf-8",
                "editor",
                "editor",
                None,
                "{}",
            )
            .expect("target");
        history
            .store
            .begin_restore(
                &root,
                "note.md",
                &before.id,
                &target.id,
                &target.content_hash,
                now_millis(),
            )
            .expect("begin");

        history
            .reconcile_pending(&root, "note.md")
            .expect("reconcile");

        assert!(history
            .store
            .pending_restores(&root, "note.md")
            .expect("pending")
            .is_empty());
    }

    #[test]
    fn restores_disk_after_saving_the_current_version() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.md");
        let root = directory.path().to_string_lossy().into_owned();
        let history = state();
        let monitor = ChangeMonitorState::default();
        fs::write(&path, "# Old").expect("write old");
        let old = read_markdown_file_sync(&root, "note.md").expect("read old");
        let target = history
            .capture_snapshot(&root, &old, "snapshot", "filesystem")
            .expect("capture old");
        fs::write(&path, "# Current").expect("write current");

        let result =
            restore_document_version_sync(&monitor, &history, &root, "note.md", &target.id)
                .expect("restore");

        assert_eq!(result.content, "# Old");
        assert_eq!(fs::read_to_string(&path).expect("read restored"), "# Old");
        let versions = history
            .store
            .list_versions(&root, "note.md")
            .expect("list versions");
        assert_eq!(versions[0].version_type, "restore");
        assert!(versions
            .iter()
            .any(|version| version.version_type == "pre_restore"));
        assert!(history
            .store
            .pending_restores(&root, "note.md")
            .expect("pending")
            .is_empty());
    }

    #[test]
    fn finishes_an_interrupted_restore_when_target_reached_disk() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.md");
        let root = directory.path().to_string_lossy().into_owned();
        let history = state();
        fs::write(&path, "# Target").expect("write target");
        let target_opened = read_markdown_file_sync(&root, "note.md").expect("read target");
        let target = history
            .capture_snapshot(&root, &target_opened, "snapshot", "filesystem")
            .expect("capture target");
        fs::write(&path, "# Before").expect("write before");
        let before_opened = read_markdown_file_sync(&root, "note.md").expect("read before");
        let before = history
            .capture_snapshot(&root, &before_opened, "snapshot", "filesystem")
            .expect("capture before");
        history
            .store
            .begin_restore(
                &root,
                "note.md",
                &before.id,
                &target.id,
                &target.content_hash,
                now_millis(),
            )
            .expect("begin");
        fs::write(&path, "# Target").expect("simulate completed disk write");

        history
            .reconcile_pending(&root, "note.md")
            .expect("reconcile");

        let versions = history.store.list_versions(&root, "note.md").expect("list");
        assert_eq!(versions[0].version_type, "restore");
        assert!(history
            .store
            .pending_restores(&root, "note.md")
            .expect("pending")
            .is_empty());
    }

    #[test]
    fn persists_pending_change_sets_across_restart_until_reviewed() {
        let directory = tempfile::tempdir().expect("temp directory");
        let database_path = directory.path().join("history.sqlite3");
        let root = directory.path().to_string_lossy().into_owned();
        let change_set_id = "cs-persistent";

        {
            let store = SqliteVersionStore::open(&database_path).expect("open store");
            store.initialize().expect("initialize");
            let history = HistoryState::new(store);
            let detail = ChangeSetDetail {
                summary: crate::change_monitor::ChangeSetSummary {
                    id: change_set_id.to_string(),
                    relative_path: "note.md".to_string(),
                    previous_relative_path: None,
                    change_type: "modified".to_string(),
                    base_version_id: "temporary-base".to_string(),
                    base_hash: content_hash(b"# Base"),
                    candidate_version_id: "temporary-candidate".to_string(),
                    candidate_hash: content_hash(b"# Candidate"),
                    status: "pending".to_string(),
                    superseded_by: None,
                    superseded_change_set_ids: Vec::new(),
                    source_type: "external".to_string(),
                    source: None,
                    agent: None,
                    reason: None,
                    detected_at: 10,
                    schema_version: 1,
                },
                base_content: "# Base".to_string(),
                candidate_content: "# Candidate".to_string(),
                base_encoding: "utf-8".to_string(),
                candidate_encoding: "utf-8".to_string(),
                base_exists: true,
                candidate_exists: true,
            };
            history
                .persist_change_set(&root, &detail)
                .expect("persist change set");
        }

        let store = SqliteVersionStore::open(&database_path).expect("reopen store");
        store.initialize().expect("reinitialize");
        let history = HistoryState::new(store);
        let pending = history.pending_change_sets(&root).expect("load pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, change_set_id);
        assert_eq!(pending[0].base_content, "# Base");
        assert_eq!(pending[0].candidate_content, "# Candidate");

        let newer_change_set_id = "cs-newer";
        history
            .persist_change_set(
                &root,
                &ChangeSetDetail {
                    summary: crate::change_monitor::ChangeSetSummary {
                        id: newer_change_set_id.to_string(),
                        relative_path: "note.md".to_string(),
                        previous_relative_path: None,
                        change_type: "modified".to_string(),
                        base_version_id: "temporary-base".to_string(),
                        base_hash: content_hash(b"# Base"),
                        candidate_version_id: "temporary-candidate".to_string(),
                        candidate_hash: content_hash(b"# Newer candidate"),
                        status: "pending".to_string(),
                        superseded_by: None,
                        superseded_change_set_ids: vec![change_set_id.to_string()],
                        source_type: "external".to_string(),
                        source: None,
                        agent: None,
                        reason: None,
                        detected_at: 20,
                        schema_version: 1,
                    },
                    base_content: "# Base".to_string(),
                    candidate_content: "# Newer candidate".to_string(),
                    base_encoding: "utf-8".to_string(),
                    candidate_encoding: "utf-8".to_string(),
                    base_exists: true,
                    candidate_exists: true,
                },
            )
            .expect("persist newer change set");
        assert_eq!(
            history
                .pending_change_sets(&root)
                .expect("load candidate chain")
                .len(),
            2
        );

        history
            .capture_action(
                &root,
                "note.md",
                "# Newer candidate",
                &content_hash(b"# Newer candidate"),
                "utf-8",
                "review_accepted",
                "review",
                None,
                &serde_json::json!({
                    "changeSetId": newer_change_set_id,
                    "supersededChangeSetIds": [change_set_id]
                })
                .to_string(),
            )
            .expect("record review");
        assert!(history
            .pending_change_sets(&root)
            .expect("reload pending")
            .is_empty());
    }

    #[test]
    fn a_discard_tombstone_clears_pending_change_sets_across_restart() {
        let directory = tempfile::tempdir().expect("temp directory");
        let root = directory.path().to_string_lossy().into_owned();
        let history = state();
        let detail = ChangeSetDetail {
            summary: crate::change_monitor::ChangeSetSummary {
                id: "cs-stuck".to_string(),
                relative_path: "note.md".to_string(),
                previous_relative_path: None,
                change_type: "modified".to_string(),
                base_version_id: "temporary-base".to_string(),
                base_hash: content_hash(b"# Base"),
                candidate_version_id: "temporary-candidate".to_string(),
                candidate_hash: content_hash(b"# Candidate"),
                status: "pending".to_string(),
                superseded_by: None,
                superseded_change_set_ids: Vec::new(),
                source_type: "external".to_string(),
                source: Some("Claude Code".to_string()),
                agent: None,
                reason: None,
                detected_at: 10,
                schema_version: 1,
            },
            base_content: "# Base".to_string(),
            candidate_content: "# Candidate".to_string(),
            base_encoding: "utf-8".to_string(),
            candidate_encoding: "utf-8".to_string(),
            base_exists: true,
            candidate_exists: true,
        };
        history
            .persist_change_set(&root, &detail)
            .expect("persist pending");
        assert_eq!(
            history.pending_change_sets(&root).expect("pending").len(),
            1
        );

        history
            .capture_action(
                &root,
                "note.md",
                "# Candidate",
                &content_hash(b"# Candidate"),
                "utf-8",
                "review_discarded",
                "review",
                Some("Claude Code".to_string()),
                &serde_json::json!({
                    "changeSetId": "cs-stuck",
                    "supersededChangeSetIds": [],
                    "discarded": true
                })
                .to_string(),
            )
            .expect("record discard");
        assert!(history
            .pending_change_sets(&root)
            .expect("pending after discard")
            .is_empty());
    }
}
