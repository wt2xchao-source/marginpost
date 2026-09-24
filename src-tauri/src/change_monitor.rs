use crate::commands::{
    canonical_workspace, content_hash, list_markdown_files_sync, read_markdown_file_sync,
    CommandError, OpenedDocument,
};
use crate::core::PendingChangeSet;
use crate::history::HistoryState;
use crate::AttributionLedger;
use notify::{
    event::ModifyKind, event::RenameMode, Event, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
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
    pub(crate) previous_relative_path: Option<String>,
    pub(crate) change_type: String,
    pub(crate) base_version_id: String,
    pub(crate) base_hash: String,
    pub(crate) candidate_version_id: String,
    pub(crate) candidate_hash: String,
    pub(crate) status: String,
    pub(crate) superseded_by: Option<String>,
    pub(crate) superseded_change_set_ids: Vec<String>,
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
    base_exists: bool,
    candidate_exists: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetDetail {
    pub(crate) summary: ChangeSetSummary,
    pub(crate) base_content: String,
    pub(crate) candidate_content: String,
    pub(crate) base_encoding: String,
    pub(crate) candidate_encoding: String,
    pub(crate) base_exists: bool,
    pub(crate) candidate_exists: bool,
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

    pub fn confirm_internal_delete(&self, relative_path: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.internal_saves.remove(relative_path);
        inner.baseline.remove(relative_path);
    }

    pub fn confirm_internal_rename(
        &self,
        previous_relative_path: &str,
        relative_path: &str,
        content: &str,
        content_hash: &str,
        encoding: &str,
    ) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.internal_saves.remove(previous_relative_path);
        inner.internal_saves.remove(relative_path);
        inner.baseline.remove(previous_relative_path);
        inner.baseline.insert(
            relative_path.to_string(),
            Snapshot {
                content: content.to_string(),
                content_hash: content_hash.to_string(),
                encoding: encoding.to_string(),
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
                    previous_relative_path: change.previous_relative_path,
                    change_type: change.change_type,
                    base_version_id: change.base_version_id,
                    base_hash: change.base_hash,
                    candidate_version_id: change.candidate_version_id,
                    candidate_hash: change.candidate_hash,
                    status: change.status,
                    superseded_by: change.superseded_by,
                    superseded_change_set_ids: change.superseded_change_set_ids,
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
                base_exists: change.base_exists,
                candidate_exists: change.candidate_exists,
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

    fn record_external_change(
        &self,
        generation: u64,
        relative_path: &str,
        previous_relative_path: Option<&str>,
        requested_change_type: &str,
        candidate: Option<Snapshot>,
        source: Option<String>,
    ) -> Option<ChangeSetSummary> {
        let mut inner = self.inner.lock().expect("change monitor state");
        if inner.generation != generation {
            return None;
        }

        if let Some(candidate) = candidate.as_ref() {
            if inner
                .internal_saves
                .get(relative_path)
                .is_some_and(|planned_hash| planned_hash == &candidate.content_hash)
            {
                inner.internal_saves.remove(relative_path);
                inner
                    .baseline
                    .insert(relative_path.to_string(), candidate.clone());
                return None;
            }
        }

        let related = inner
            .changes
            .iter()
            .filter(|change| {
                change.summary.status == "pending"
                    && paths_overlap(&change.summary, relative_path, previous_relative_path)
            })
            .map(|change| change.summary.id.clone())
            .collect::<Vec<_>>();
        let previous_record = inner
            .changes
            .iter()
            .rev()
            .find(|change| paths_overlap(&change.summary, relative_path, previous_relative_path))
            .cloned();
        let base_path = previous_record
            .as_ref()
            .and_then(|change| {
                change
                    .summary
                    .previous_relative_path
                    .as_deref()
                    .or(Some(change.summary.relative_path.as_str()))
            })
            .or(previous_relative_path)
            .unwrap_or(relative_path);
        let (base, base_exists) = if let Some(change) = previous_record.as_ref() {
            (
                Snapshot {
                    content: change.base_content.clone(),
                    content_hash: change.summary.base_hash.clone(),
                    encoding: change.base_encoding.clone(),
                },
                change.base_exists,
            )
        } else if let Some(base) = inner.baseline.get(base_path).cloned() {
            (base, true)
        } else {
            (
                Snapshot {
                    content: String::new(),
                    content_hash: content_hash(&[]),
                    encoding: "utf-8".to_string(),
                },
                false,
            )
        };
        let candidate_exists = candidate.is_some();
        let candidate = candidate.unwrap_or_else(|| Snapshot {
            content: String::new(),
            content_hash: content_hash(&[]),
            encoding: base.encoding.clone(),
        });

        if base_exists == candidate_exists
            && base.content_hash == candidate.content_hash
            && base_path == relative_path
            || inner.changes.iter().any(|change| {
                change.summary.relative_path == relative_path
                    && change.summary.candidate_hash == candidate.content_hash
                    && change.candidate_exists == candidate_exists
            })
        {
            return None;
        }

        inner.next_id = inner.next_id.wrapping_add(1);
        let id = format!("cs-{}-{}", now_millis(), inner.next_id);
        for change in &mut inner.changes {
            if related.contains(&change.summary.id) {
                change.summary.status = "superseded".to_string();
                change.summary.superseded_by = Some(id.clone());
            }
        }
        let change_type = previous_record
            .as_ref()
            .map(|change| change.summary.change_type.clone())
            .unwrap_or_else(|| requested_change_type.to_string());
        let summary = ChangeSetSummary {
            id,
            relative_path: relative_path.to_string(),
            previous_relative_path: previous_record
                .as_ref()
                .and_then(|change| change.summary.previous_relative_path.clone())
                .or_else(|| previous_relative_path.map(str::to_string)),
            change_type,
            base_version_id: format!("version-{}", base.content_hash),
            base_hash: base.content_hash.clone(),
            candidate_version_id: format!("version-{}", candidate.content_hash),
            candidate_hash: candidate.content_hash.clone(),
            status: "pending".to_string(),
            superseded_by: None,
            superseded_change_set_ids: related,
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
            base_exists,
            candidate_exists,
        });
        Some(summary)
    }

    #[cfg(test)]
    fn record_external_candidate(
        &self,
        generation: u64,
        relative_path: &str,
        candidate: Snapshot,
        source: Option<String>,
    ) -> Option<ChangeSetSummary> {
        let change_type = if self
            .inner
            .lock()
            .expect("state")
            .baseline
            .contains_key(relative_path)
        {
            "modified"
        } else {
            "created"
        };
        self.record_external_change(
            generation,
            relative_path,
            None,
            change_type,
            Some(candidate),
            source,
        )
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
                base_exists: change.base_exists,
                candidate_exists: change.candidate_exists,
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
                        base_exists: change.base_exists,
                        candidate_exists: change.candidate_exists,
                    },
                )
            })
    }

    pub(crate) fn actionable_review_context(
        &self,
        id: &str,
    ) -> Result<(String, ChangeSetDetail), CommandError> {
        let context = self.review_context(id).ok_or_else(|| {
            CommandError::new(
                "CHANGE_SET_UNAVAILABLE",
                "This change set is no longer available.",
            )
        })?;
        if context.1.summary.status != "pending" {
            return Err(CommandError::new(
                "CHANGE_SET_SUPERSEDED",
                "A newer change set replaced this one.",
            ));
        }
        Ok(context)
    }

    pub(crate) fn remove_change(&self, id: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        inner.changes.retain(|change| change.summary.id != id);
    }

    pub(crate) fn mark_stale(&self, id: &str) {
        let mut inner = self.inner.lock().expect("change monitor state");
        if let Some(change) = inner
            .changes
            .iter_mut()
            .find(|change| change.summary.id == id)
        {
            change.summary.status = "stale".to_string();
        }
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

fn paths_overlap(
    summary: &ChangeSetSummary,
    relative_path: &str,
    previous_relative_path: Option<&str>,
) -> bool {
    let mut existing = vec![summary.relative_path.as_str()];
    if let Some(previous) = summary.previous_relative_path.as_deref() {
        existing.push(previous);
    }
    existing.contains(&relative_path)
        || previous_relative_path.is_some_and(|path| existing.contains(&path))
}

fn normalize_markdown_path(root: &Path, path: &Path) -> Option<String> {
    if !path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    }) {
        return None;
    }
    let relative = path.strip_prefix(root).ok()?;
    if path_is_ignored(relative) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WorkspaceFileEvent {
    Changed(String),
    Removed(String),
    Renamed { from: String, to: String },
}

fn normalize_event(root: &Path, event: Event) -> Vec<WorkspaceFileEvent> {
    if matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
    ) && event.paths.len() >= 2
    {
        let from = normalize_markdown_path(root, &event.paths[0]);
        let to = normalize_markdown_path(root, &event.paths[1]);
        return match (from, to) {
            (Some(from), Some(to)) => vec![WorkspaceFileEvent::Renamed { from, to }],
            (Some(from), None) => vec![WorkspaceFileEvent::Removed(from)],
            (None, Some(to)) => vec![WorkspaceFileEvent::Changed(to)],
            (None, None) => Vec::new(),
        };
    }

    event
        .paths
        .into_iter()
        .filter_map(|path| {
            let relative = normalize_markdown_path(root, &path)?;
            Some(
                if matches!(event.kind, EventKind::Remove(_))
                    || matches!(
                        event.kind,
                        EventKind::Modify(ModifyKind::Name(RenameMode::From))
                    )
                {
                    WorkspaceFileEvent::Removed(relative)
                } else {
                    WorkspaceFileEvent::Changed(relative)
                },
            )
        })
        .collect()
}

fn process_events(
    app: &AppHandle,
    state: &ChangeMonitorState,
    generation: u64,
    root_path: &str,
    events: HashSet<WorkspaceFileEvent>,
) {
    let mut events = events.into_iter().collect::<Vec<_>>();
    let explicit_renames = events
        .iter()
        .filter_map(|event| match event {
            WorkspaceFileEvent::Renamed { from, to } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    events.retain(|event| {
        !explicit_renames.iter().any(|(from, to)| {
            matches!(event, WorkspaceFileEvent::Removed(path) if path == from)
                || matches!(event, WorkspaceFileEvent::Changed(path) if path == to)
        }) || matches!(event, WorkspaceFileEvent::Renamed { .. })
    });
    if explicit_renames.is_empty() {
        let removed = events
            .iter()
            .filter_map(|event| match event {
                WorkspaceFileEvent::Removed(path) => Some(path.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let created = events
            .iter()
            .filter_map(|event| match event {
                WorkspaceFileEvent::Changed(path) => {
                    let inner = state.inner.lock().expect("state");
                    (!inner.baseline.contains_key(path)).then_some(path.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if removed.len() == 1 && created.len() == 1 {
            let from = removed[0].clone();
            let to = created[0].clone();
            if state
                .inner
                .lock()
                .expect("state")
                .baseline
                .contains_key(&from)
            {
                events.retain(|event| {
                    !matches!(event, WorkspaceFileEvent::Removed(path) if path == &from)
                        && !matches!(event, WorkspaceFileEvent::Changed(path) if path == &to)
                });
                events.push(WorkspaceFileEvent::Renamed { from, to });
            }
        }
    }
    events.sort_by_key(|event| match event {
        WorkspaceFileEvent::Renamed { .. } => 0,
        WorkspaceFileEvent::Removed(_) => 1,
        WorkspaceFileEvent::Changed(_) => 2,
    });
    for event in events {
        let (relative_path, previous_relative_path, change_type, candidate) = match event {
            WorkspaceFileEvent::Changed(relative_path) => {
                let Ok(opened) = read_markdown_file_sync(root_path, &relative_path) else {
                    continue;
                };
                let change_type = if state
                    .inner
                    .lock()
                    .expect("state")
                    .baseline
                    .contains_key(&relative_path)
                {
                    "modified"
                } else {
                    "created"
                };
                (relative_path, None, change_type, Some(opened.into()))
            }
            WorkspaceFileEvent::Removed(relative_path) => (relative_path, None, "deleted", None),
            WorkspaceFileEvent::Renamed { from, to } => {
                let Ok(opened) = read_markdown_file_sync(root_path, &to) else {
                    continue;
                };
                (to, Some(from), "renamed", Some(opened.into()))
            }
        };
        let attributed_source = app
            .state::<AttributionLedger>()
            .take_match(&Path::new(root_path).join(&relative_path), now_millis());
        if let Some(change) = state.record_external_change(
            generation,
            &relative_path,
            previous_relative_path.as_deref(),
            change_type,
            candidate,
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

        let mut events = normalize_event(&root, first_event)
            .into_iter()
            .collect::<HashSet<_>>();
        let started = std::time::Instant::now();
        while let Some(remaining) = DEBOUNCE_WINDOW.checked_sub(started.elapsed()) {
            match event_receiver.recv_timeout(remaining) {
                Ok(Ok(event)) => events.extend(normalize_event(&root, event)),
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let state = app.state::<ChangeMonitorState>();
        process_events(&app, &state, generation, &root_path, events);
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
        assert_eq!(
            state.get_change(&first.id).expect("first").summary.status,
            "superseded"
        );
        assert_eq!(
            state
                .get_change(&first.id)
                .expect("first")
                .summary
                .superseded_by
                .as_deref(),
            Some(second.id.as_str())
        );
        assert_eq!(second.superseded_change_set_ids, vec![first.id]);
    }

    #[test]
    fn marks_a_conflicted_change_set_as_stale() {
        let state = state_with_baseline("note.md", "# Base");
        let generation = state.inner.lock().expect("state").generation;
        let change = state
            .record_external_candidate(generation, "note.md", snapshot("# Candidate"), None)
            .expect("candidate");

        state.mark_stale(&change.id);

        assert_eq!(
            state.get_change(&change.id).expect("change").summary.status,
            "stale"
        );
    }

    #[test]
    fn records_deleted_and_renamed_files_with_existence_semantics() {
        let deleted_state = state_with_baseline("note.md", "# Base");
        let generation = deleted_state.inner.lock().expect("state").generation;
        let deleted = deleted_state
            .record_external_change(generation, "note.md", None, "deleted", None, None)
            .expect("deleted change");
        let deleted_detail = deleted_state
            .get_change(&deleted.id)
            .expect("deleted detail");
        assert_eq!(deleted.change_type, "deleted");
        assert!(deleted_detail.base_exists);
        assert!(!deleted_detail.candidate_exists);

        let renamed_state = state_with_baseline("before.md", "# Base");
        let generation = renamed_state.inner.lock().expect("state").generation;
        let renamed = renamed_state
            .record_external_change(
                generation,
                "after.md",
                Some("before.md"),
                "renamed",
                Some(snapshot("# Base")),
                None,
            )
            .expect("renamed change");
        assert_eq!(renamed.change_type, "renamed");
        assert_eq!(renamed.previous_relative_path.as_deref(), Some("before.md"));
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
                previous_relative_path: None,
                change_type: "modified".to_string(),
                status: "pending".to_string(),
                superseded_by: None,
                superseded_change_set_ids: Vec::new(),
                base_exists: true,
                candidate_exists: true,
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
