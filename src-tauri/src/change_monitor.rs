use crate::commands::{
    canonical_workspace, content_hash, list_markdown_files_sync, read_markdown_file_sync,
    CommandError, OpenedDocument,
};
use crate::core::PendingChangeSet;
use crate::history::HistoryState;
use crate::AttributionLedger;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

const CHANGE_SET_CREATED_EVENT: &str = "change-set-created";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
struct Snapshot {
    content: String,
    content_hash: String,
    encoding: String,
}

impl From<OpenedDocument> for Snapshot {
    fn from(document: OpenedDocument) -> Self {
        Self {
            content: document.content,
            content_hash: document.content_hash,
            encoding: document.encoding,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetSummary {
    pub(crate) id: String,
    pub(crate) relative_path: String,
    pub(crate) change_type: String,
    pub(crate) base_version_id: String,
    pub(crate) base_hash: String,
    pub(crate) candidate_version_id: String,
    pub(crate) candidate_hash: String,
    pub(crate) status: String,
    pub(crate) source_type: String,
    pub(crate) source: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) detected_at: u64,
    pub(crate) schema_version: u32,
}

#[derive(Debug, Clone)]
struct ChangeSetRecord {
    summary: ChangeSetSummary,
    base_content: String,
    candidate_content: String,
    base_encoding: String,
    candidate_encoding: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetDetail {
    pub(crate) summary: ChangeSetSummary,
    pub(crate) base_content: String,
    pub(crate) candidate_content: String,
    pub(crate) base_encoding: String,
    pub(crate) candidate_encoding: String,
}

#[derive(Default)]
struct ChangeMonitorInner {
    generation: u64,
    root_path: Option<PathBuf>,
    baseline: HashMap<String, Snapshot>,
    changes: Vec<ChangeSetRecord>,
    internal_saves: HashMap<String, String>,
    next_id: u64,
    stop_sender: Option<mpsc::Sender<()>>,
}

#[derive(Default)]
pub struct ChangeMonitorState {
    inner: Mutex<ChangeMonitorInner>,
}

impl ChangeMonitorState {
    pub fn prepare_internal_save(&self, relative_path: &str, content_hash: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner
            .internal_saves
            .insert(relative_path.to_string(), content_hash.to_string());
    }

    pub fn confirm_internal_save(&self, relative_path: &str, content: &str, content_hash: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.internal_saves.remove(relative_path);
        let encoding = inner
            .baseline
            .get(relative_path)
            .map(|snapshot| snapshot.encoding.clone())
            .unwrap_or_else(|| "utf-8".to_string());
        inner.baseline.insert(
            relative_path.to_string(),
            Snapshot {
                content: content.to_string(),
                content_hash: content_hash.to_string(),
                encoding,
            },
        );
    }

    pub fn cancel_internal_save(&self, relative_path: &str, content_hash: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        if inner.internal_saves.get(relative_path).map(String::as_str) == Some(content_hash) {
            inner.internal_saves.remove(relative_path);
        }
    }

    fn stop_current(&self) {
        let sender = {
            let mut inner = self.inner.lock().expect("change monitor state");
            inner.stop_sender.take()
        };
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }

    fn reset(
        &self,
        root_path: PathBuf,
        baseline: HashMap<String, Snapshot>,
        stop_sender: mpsc::Sender<()>,
    ) -> u64 {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.generation = inner.generation.wrapping_add(1);
        inner.root_path = Some(root_path);
        inner.baseline = baseline;
        inner.changes.clear();
        inner.internal_saves.clear();
        inner.next_id = 0;
        inner.stop_sender = Some(stop_sender);
        inner.generation
    }

    fn restore_pending(&self, generation: u64, pending: Vec<PendingChangeSet>) {
        let mut inner = self.inner.lock().expect("change monitor state");
        if inner.generation != generation {
            return;
        }
        inner.changes = pending
            .into_iter()
            .map(|change| ChangeSetRecord {
                summary: ChangeSetSummary {
                    id: change.id,
                    relative_path: change.relative_path.clone(),
                    change_type: if change.base_content.is_empty() {
                        "created".to_string()
                    } else {
                        "modified".to_string()
                    },
                    base_version_id: change.base_version_id,
                    base_hash: change.base_hash,
                    candidate_version_id: change.candidate_version_id,
                    candidate_hash: change.candidate_hash,
                    status: "pending".to_string(),
                    source_type: "external".to_string(),
                    source: change.source.clone(),
                    agent: None,
                    reason: None,
                    detected_at: change.detected_at,
                    schema_version: 1,
                },
                base_content: change.base_content,
                candidate_content: change.candidate_content,
                base_encoding: change.base_encoding,
                candidate_encoding: change.candidate_encoding,
            })
            .collect();
    }

    pub(crate) fn forget_document(&self, relative_path: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.baseline.remove(relative_path);
        inner.internal_saves.remove(relative_path);
    }

    fn set_version_ids(&self, id: &str, base_version_id: &str, candidate_version_id: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        if let Some(change) = inner
            .changes
            .iter_mut()
            .find(|change| change.summary.id == id)
        {
            change.summary.base_version_id = base_version_id.to_string();
            change.summary.candidate_version_id = candidate_version_id.to_string();
        }
    }

    fn record_external_candidate(
        &self,
        generation: u64,
        relative_path: &str,
        candidate: Snapshot,
        source: Option<String>,
    ) -> Option<ChangeSetSummary> {
        let mut inner = self.inner.lock().expect("change monitor state");
        if inner.generation != generation {
            return None;
        }

        if inner
            .internal_saves
            .get(relative_path)
            .is_some_and(|planned_hash| planned_hash == &candidate.content_hash)
        {
            inner.internal_saves.remove(relative_path);
            inner.baseline.insert(relative_path.to_string(), candidate);
            return None;
        }

        let base = inner
            .baseline
            .get(relative_path)
            .cloned()
            .unwrap_or_else(|| Snapshot {
                content: String::new(),
                content_hash: content_hash(&[]),
                encoding: "utf-8".to_string(),
            });

        if base.content_hash == candidate.content_hash
            || inner.changes.iter().any(|change| {
                change.summary.relative_path == relative_path
                    && change.summary.candidate_hash == candidate.content_hash
            })
        {
            return None;
        }

        inner.next_id = inner.next_id.wrapping_add(1);
        let summary = ChangeSetSummary {
            id: format!("cs-{}-{}", now_millis(), inner.next_id),
            relative_path: relative_path.to_string(),
            change_type: if inner.baseline.contains_key(relative_path) {
                "modified".to_string()
            } else {
                "created".to_string()
            },
            base_version_id: format!("version-{}", base.content_hash),
            base_hash: base.content_hash.clone(),
            candidate_version_id: format!("version-{}", candidate.content_hash),
            candidate_hash: candidate.content_hash.clone(),
            status: "pending".to_string(),
            source_type: "external".to_string(),
            source,
            agent: None,
            reason: None,
            detected_at: now_millis(),
            schema_version: 1,
        };
        inner.changes.push(ChangeSetRecord {
            summary: summary.clone(),
            base_content: base.content,
            candidate_content: candidate.content,
            base_encoding: base.encoding,
            candidate_encoding: candidate.encoding,
        });
        Some(summary)
    }

    fn list_changes(&self) -> Vec<ChangeSetSummary> {
        let inner = self.inner.lock().expect("change monitor state");
        inner
            .changes
            .iter()
            .rev()
            .map(|change| change.summary.clone())
            .collect()
    }

    pub(crate) fn get_change(&self, id: &str) -> Option<ChangeSetDetail> {
        let inner = self.inner.lock().expect("change monitor state");
        inner
            .changes
            .iter()
            .find(|change| change.summary.id == id)
            .map(|change| ChangeSetDetail {
                summary: change.summary.clone(),
                base_content: change.base_content.clone(),
                candidate_content: change.candidate_content.clone(),
                base_encoding: change.base_encoding.clone(),
                candidate_encoding: change.candidate_encoding.clone(),
            })
    }

    pub(crate) fn review_context(&self, id: &str) -> Option<(String, ChangeSetDetail)> {
        let inner = self.inner.lock().expect("change monitor state");
        let root_path = inner.root_path.as_ref()?.to_string_lossy().into_owned();
        inner
            .changes
            .iter()
            .find(|change| change.summary.id == id)
            .map(|change| {
                (
                    root_path,
                    ChangeSetDetail {
                        summary: change.summary.clone(),
                        base_content: change.base_content.clone(),
                        candidate_content: change.candidate_content.clone(),
                        base_encoding: change.base_encoding.clone(),
                        candidate_encoding: change.candidate_encoding.clone(),
                    },
                )
            })
    }

    pub(crate) fn remove_change(&self, id: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.changes.retain(|change| change.summary.id != id);
    }

    pub(crate) fn change_ids_for_path(&self, relative_path: &str) -> Vec<String> {
        let inner = self.inner.lock().expect("change monitor state");
        inner
            .changes
            .iter()
            .filter(|change| change.summary.relative_path == relative_path)
            .map(|change| change.summary.id.clone())
            .collect()
    }

    pub(crate) fn remove_changes(&self, ids: &[String]) {
        let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut inner = self.inner.lock().expect("change monitor state");
        inner
            .changes
            .retain(|change| !ids.contains(change.summary.id.as_str()));
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn initial_baseline(root_path: &str) -> Result<HashMap<String, Snapshot>, CommandError> {
    let mut baseline = HashMap::new();
    for document in list_markdown_files_sync(root_path)? {
        if let Ok(opened) = read_markdown_file_sync(root_path, &document.relative_path) {
            baseline.insert(document.relative_path, opened.into());
        }
    }
    Ok(baseline)
}

fn path_is_ignored(relative_path: &Path) -> bool {
    relative_path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        [".git", "node_modules", "target", "dist"]
            .iter()
            .any(|ignored| name.eq_ignore_ascii_case(ignored))
    })
}

fn normalize_event_paths(root: &Path, event: Event) -> HashSet<String> {
    event
        .paths
        .into_iter()
        .filter_map(|path| {
            if !path.is_file()
                || !path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("md")
                        || extension.eq_ignore_ascii_case("markdown")
                })
            {
                return None;
            }
            let relative = path.strip_prefix(root).ok()?;
            if path_is_ignored(relative) {
                return None;
            }
            Some(relative.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

fn process_paths(
    app: &AppHandle,
    state: &ChangeMonitorState,
    generation: u64,
    root_path: &str,
    paths: HashSet<String>,
) {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    for relative_path in paths {
        let Ok(opened) = read_markdown_file_sync(root_path, &relative_path) else {
            continue;
        };
        let attributed_source = app
            .state::<AttributionLedger>()
            .take_match(&Path::new(root_path).join(&relative_path), now_millis());
        if let Some(change) = state.record_external_candidate(
            generation,
            &relative_path,
            opened.into(),
            attributed_source,
        ) {
            let Some(detail) = state.get_change(&change.id) else {
                continue;
            };
            let history = app.state::<HistoryState>();
            match history.persist_change_set(root_path, &detail) {
                Ok((base_version_id, candidate_version_id)) => {
                    state.set_version_ids(&change.id, &base_version_id, &candidate_version_id);
                }
                Err(_) => {
                    state.remove_change(&change.id);
                    continue;
                }
            }
            if let Some(persisted) = state.get_change(&change.id) {
                let _ = app.emit(CHANGE_SET_CREATED_EVENT, persisted.summary);
            }
        }
    }
}

fn watcher_loop(
    app: AppHandle,
    generation: u64,
    root: PathBuf,
    root_path: String,
    event_receiver: mpsc::Receiver<notify::Result<Event>>,
    stop_receiver: mpsc::Receiver<()>,
    _watcher: RecommendedWatcher,
) {
    loop {
        if stop_receiver.try_recv().is_ok() {
            break;
        }

        let first_event = match event_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => event,
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mut paths = normalize_event_paths(&root, first_event);
        let started = std::time::Instant::now();
        while let Some(remaining) = DEBOUNCE_WINDOW.checked_sub(started.elapsed()) {
            match event_receiver.recv_timeout(remaining) {
                Ok(Ok(event)) => paths.extend(normalize_event_paths(&root, event)),
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let state = app.state::<ChangeMonitorState>();
        process_paths(&app, &state, generation, &root_path, paths);
    }
}

#[tauri::command]
pub fn start_workspace_watch(
    app: AppHandle,
    state: State<'_, ChangeMonitorState>,
    history: State<'_, HistoryState>,
    root_path: String,
) -> Result<(), CommandError> {
    state.stop_current();
    let root = canonical_workspace(&root_path)?;
    let canonical_root_path = root.to_string_lossy().into_owned();
    let baseline = initial_baseline(&canonical_root_path)?;
    let (event_sender, event_receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result| {
        let _ = event_sender.send(result);
    })
    .map_err(|error| CommandError::new("WATCH_START_FAILED", error.to_string()))?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|error| CommandError::new("WATCH_START_FAILED", error.to_string()))?;

    let (stop_sender, stop_receiver) = mpsc::channel();
    let generation = state.reset(root.clone(), baseline, stop_sender);
    state.restore_pending(
        generation,
        history.pending_change_sets(&canonical_root_path)?,
    );
    thread::spawn(move || {
        watcher_loop(
            app,
            generation,
            root,
            canonical_root_path,
            event_receiver,
            stop_receiver,
            watcher,
        )
    });

    Ok(())
}

#[tauri::command]
pub fn stop_workspace_watch(state: State<'_, ChangeMonitorState>) {
    state.stop_current();
}

#[tauri::command]
pub fn list_change_sets(state: State<'_, ChangeMonitorState>) -> Vec<ChangeSetSummary> {
    state.list_changes()
}

#[tauri::command]
pub fn get_change_set(state: State<'_, ChangeMonitorState>, id: String) -> Option<ChangeSetDetail> {
    state.get_change(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(content: &str) -> Snapshot {
        Snapshot {
            content: content.to_string(),
            content_hash: content_hash(content.as_bytes()),
            encoding: "utf-8".to_string(),
        }
    }

    fn state_with_baseline(relative_path: &str, content: &str) -> ChangeMonitorState {
        let state = ChangeMonitorState::default();
        let (stop_sender, _stop_receiver) = mpsc::channel();
        state.reset(
            PathBuf::from("/workspace"),
            HashMap::from([(relative_path.to_string(), snapshot(content))]),
            stop_sender,
        );
        state
    }

    #[test]
    fn creates_one_change_set_for_duplicate_candidate_events() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;

        assert!(state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate"), None)
            .is_some());
        assert!(state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate"), None)
            .is_none());
        assert_eq!(state.list_changes().len(), 1);
    }

    #[test]
    fn ignores_content_that_matches_the_baseline() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;

        assert!(state
            .record_external_candidate(generation, "note.md", snapshot("# Base"), None)
            .is_none());
        assert!(state.list_changes().is_empty());
    }

    #[test]
    fn excludes_an_expected_internal_save() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;
        let candidate = snapshot("# Editor save");
        state.prepare_internal_save("note.md", &candidate.content_hash);

        assert!(state
            .record_external_candidate(generation, "note.md", candidate, None)
            .is_none());
        assert!(state.list_changes().is_empty());
    }

    #[test]
    fn creates_a_change_set_for_a_new_markdown_file() {
        let state = ChangeMonitorState::default();
        let (stop_sender, _stop_receiver) = mpsc::channel();
        let generation = state.reset(PathBuf::from("/workspace"), HashMap::new(), stop_sender);

        let change = state
            .record_external_candidate(generation, "new.md", snapshot("# New"), None)
            .expect("new file change");

        assert_eq!(change.change_type, "created");
        let inner = state.inner.lock().expect("state");
        assert_eq!(inner.changes[0].base_content, "");
        assert_eq!(inner.changes[0].candidate_content, "# New");
    }

    #[test]
    fn keeps_the_same_base_for_sequential_external_candidates() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;
        let first = state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate one"), None)
            .expect("first candidate");
        let second = state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate two"), None)
            .expect("second candidate");

        assert_eq!(first.base_hash, second.base_hash);
        assert_ne!(first.candidate_hash, second.candidate_hash);
    }

    #[test]
    fn restores_persisted_pending_changes_after_monitor_reset() {
        let state = state_with_baseline("note.md", "# Candidate");
        let generation = state.inner.lock().expect("state").generation;
        state.restore_pending(
            generation,
            vec![PendingChangeSet {
                id: "cs-persisted".to_string(),
                relative_path: "note.md".to_string(),
                base_version_id: "version-base".to_string(),
                base_content: "# Base".to_string(),
                base_hash: content_hash(b"# Base"),
                base_encoding: "utf-8".to_string(),
                candidate_version_id: "version-candidate".to_string(),
                candidate_content: "# Candidate".to_string(),
                candidate_hash: content_hash(b"# Candidate"),
                candidate_encoding: "utf-8".to_string(),
                source: Some("Claude Code".to_string()),
                detected_at: 1,
            }],
        );

        let restored = state.get_change("cs-persisted").expect("restored change");
        assert_eq!(restored.base_content, "# Base");
        assert_eq!(restored.candidate_content, "# Candidate");
        assert_eq!(restored.summary.source.as_deref(), Some("Claude Code"));
        assert_eq!(state.list_changes().len(), 1);
    }

    #[test]
    fn carries_a_self_reported_source_onto_the_change_set() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;

        let change = state
            .record_external_candidate(
                generation,
                "note.md",
                snapshot("# Candidate"),
                Some("Claude Code".to_string()),
            )
            .expect("attributed change");

        assert_eq!(change.source.as_deref(), Some("Claude Code"));
        assert_eq!(
            state.list_changes()[0].source.as_deref(),
            Some("Claude Code")
        );
    }

    #[test]
    fn removes_a_resolved_candidate_chain_for_one_document() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;
        state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate one"), None)
            .expect("first candidate");
        state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate two"), None)
            .expect("second candidate");

        let ids = state.change_ids_for_path("note.md");
        assert_eq!(ids.len(), 2);
        state.remove_changes(&ids);
        assert!(state.list_changes().is_empty());
    }
}
