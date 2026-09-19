mod change_monitor;
mod commands;
pub mod core;
mod history;
mod review;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const NOTIFY_FLAG: &str = "--marginpost-notify";
const ATTRIBUTE_FLAG: &str = "--marginpost-attribute";
const AGENT_SESSION_ENDED_EVENT: &str = "agent-session-ended";
const ATTRIBUTION_RETENTION_MS: u64 = 600_000;
const ATTRIBUTION_MATCH_WINDOW_MS: u64 = 120_000;
const ATTRIBUTION_MATCH_TOLERANCE_MS: u64 = 10_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionEnded {
    pub(crate) source: String,
}

#[derive(Default)]
pub(crate) struct AgentNotifyFlag(pub(crate) Mutex<Option<String>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributionEntry {
    pub(crate) source: String,
    pub(crate) path: PathBuf,
    pub(crate) at_millis: u64,
}

#[derive(Default)]
pub(crate) struct AttributionLedger(pub(crate) Mutex<Vec<AttributionEntry>>);

impl AttributionLedger {
    pub(crate) fn record(&self, source: String, path: PathBuf) {
        let now = now_millis();
        let mut entries = self.0.lock().expect("attribution ledger");
        entries.push(AttributionEntry {
            source,
            path,
            at_millis: now,
        });
        entries.retain(|entry| now.saturating_sub(entry.at_millis) <= ATTRIBUTION_RETENTION_MS);
    }

    /// Consumes the most recent self-reported entry whose path matches and
    /// whose timestamp falls inside the matching window around `now`.
    pub(crate) fn take_match(&self, target: &Path, now: u64) -> Option<String> {
        let normalized_target = normalize_path(target);
        let mut entries = self.0.lock().expect("attribution ledger");
        entries.retain(|entry| now.saturating_sub(entry.at_millis) <= ATTRIBUTION_RETENTION_MS);
        let index = entries.iter().rposition(|entry| {
            normalize_path(&entry.path) == normalized_target
                && now + ATTRIBUTION_MATCH_TOLERANCE_MS >= entry.at_millis
                && now.saturating_sub(entry.at_millis) <= ATTRIBUTION_MATCH_WINDOW_MS
        })?;
        Some(entries.remove(index).source)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[tauri::command]
fn technical_baseline() -> serde_json::Value {
    serde_json::json!({
        "stage": "AMR-011",
        "file_source": "local",
        "change_detector": "notify",
        "diff_engine": "similar-structured-markdown",
        "version_store": "sqlite-schema-v2"
    })
}

fn extract_notify_source(args: &[String]) -> Option<String> {
    let position = args.iter().position(|arg| arg == NOTIFY_FLAG)?;
    let fallback = args
        .get(position + 1)
        .is_some_and(|next| !next.starts_with("--"))
        .then(|| args[position + 1].clone());
    Some(fallback.unwrap_or_else(|| "Agent".to_string()))
}

fn extract_attribute(args: &[String]) -> Option<(String, String)> {
    let position = args.iter().position(|arg| arg == ATTRIBUTE_FLAG)?;
    let source = args.get(position + 1)?;
    let path = args.get(position + 2)?;
    if source.starts_with("--") || source.is_empty() || path.starts_with("--") || path.is_empty() {
        return None;
    }
    Some((source.clone(), path.clone()))
}

#[tauri::command]
fn take_agent_notify(state: tauri::State<AgentNotifyFlag>) -> Option<String> {
    state.0.lock().expect("agent notify lock").take()
}

#[tauri::command]
fn update_dock_badge(app: AppHandle, count: i32) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    window
        .set_badge_count((count > 0).then_some(i64::from(count)))
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(change_monitor::ChangeMonitorState::default())
        .manage(AgentNotifyFlag::default())
        .manage(AttributionLedger::default())
        .plugin(tauri_plugin_single_instance::init(
            |app: &AppHandle, args: Vec<String>, _cwd: String| {
                if let Some((source, path)) = extract_attribute(&args) {
                    app.state::<AttributionLedger>()
                        .record(source, PathBuf::from(path));
                }
                if let Some(source) = extract_notify_source(&args) {
                    let _ = app.emit(AGENT_SESSION_ENDED_EVENT, AgentSessionEnded { source });
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let launch_args = std::env::args().collect::<Vec<_>>();
            let launch_source = extract_notify_source(&launch_args);
            if launch_source.is_some() {
                let state = app.state::<AgentNotifyFlag>();
                *state.0.lock().expect("agent notify lock") = launch_source;
            }
            if let Some((source, path)) = extract_attribute(&launch_args) {
                app.state::<AttributionLedger>()
                    .record(source, PathBuf::from(path));
            }
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = core::SqliteVersionStore::open(&data_dir.join("history.sqlite3"))
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            core::VersionStore::initialize(&store)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(history::HistoryState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            technical_baseline,
            commands::list_markdown_files,
            commands::inspect_dropped_path,
            commands::read_markdown_file,
            commands::save_markdown_file,
            take_agent_notify,
            update_dock_badge,
            change_monitor::start_workspace_watch,
            change_monitor::stop_workspace_watch,
            change_monitor::list_change_sets,
            change_monitor::get_change_set,
            review::get_change_set_review,
            review::resolve_change_set,
            review::discard_change_set,
            history::list_document_versions,
            history::get_document_version,
            history::restore_document_version
        ])
        .run(tauri::generate_context!())
        .expect("failed to run MarginPost");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_named_notify_source() {
        let args = vec![
            "/Applications/MarginPost.app/Contents/MacOS/MarginPost".to_string(),
            NOTIFY_FLAG.to_string(),
            "Claude Code".to_string(),
        ];
        assert_eq!(extract_notify_source(&args).as_deref(), Some("Claude Code"));
    }

    #[test]
    fn falls_back_to_a_generic_notify_source() {
        let args = vec![NOTIFY_FLAG.to_string()];
        assert_eq!(extract_notify_source(&args).as_deref(), Some("Agent"));
    }

    #[test]
    fn ignores_a_launch_without_the_notify_flag() {
        let args = vec![
            "/Applications/MarginPost.app/Contents/MacOS/MarginPost".to_string(),
            "Claude Code".to_string(),
        ];
        assert_eq!(extract_notify_source(&args), None);
    }

    #[test]
    fn does_not_treat_another_flag_as_the_source() {
        let args = vec![NOTIFY_FLAG.to_string(), "--other".to_string()];
        assert_eq!(extract_notify_source(&args).as_deref(), Some("Agent"));
    }

    #[test]
    fn extracts_an_attribute_pair() {
        let args = vec![
            "/Applications/MarginPost.app/Contents/MacOS/MarginPost".to_string(),
            ATTRIBUTE_FLAG.to_string(),
            "Claude Code".to_string(),
            "/workspace/notes/product.md".to_string(),
        ];
        assert_eq!(
            extract_attribute(&args),
            Some((
                "Claude Code".to_string(),
                "/workspace/notes/product.md".to_string()
            ))
        );
    }

    #[test]
    fn rejects_incomplete_attribute_flags() {
        let only_flag = vec![ATTRIBUTE_FLAG.to_string()];
        assert_eq!(extract_attribute(&only_flag), None);
        let missing_path = vec![ATTRIBUTE_FLAG.to_string(), "Claude Code".to_string()];
        assert_eq!(extract_attribute(&missing_path), None);
        let flag_like_path = vec![
            ATTRIBUTE_FLAG.to_string(),
            "Claude Code".to_string(),
            "--other".to_string(),
        ];
        assert_eq!(extract_attribute(&flag_like_path), None);
    }

    #[test]
    fn ledger_matches_and_consumes_the_most_recent_entry() {
        let ledger = AttributionLedger::default();
        let base = 1_000_000_u64;
        {
            let mut entries = ledger.0.lock().expect("ledger");
            entries.push(AttributionEntry {
                source: "Codex".to_string(),
                path: PathBuf::from("/workspace/a.md"),
                at_millis: base,
            });
            entries.push(AttributionEntry {
                source: "Claude Code".to_string(),
                path: PathBuf::from("/workspace/a.md"),
                at_millis: base + 500,
            });
        }

        let now = base + 1_000;
        assert_eq!(
            ledger.take_match(Path::new("/workspace/a.md"), now),
            Some("Claude Code".to_string())
        );
        assert_eq!(
            ledger.take_match(Path::new("/workspace/a.md"), now),
            Some("Codex".to_string())
        );
        assert_eq!(ledger.take_match(Path::new("/workspace/a.md"), now), None);
    }

    #[test]
    fn ledger_rejects_stale_entries_and_foreign_paths() {
        let ledger = AttributionLedger::default();
        let base = 1_000_000_u64;
        {
            let mut entries = ledger.0.lock().expect("ledger");
            entries.push(AttributionEntry {
                source: "Claude Code".to_string(),
                path: PathBuf::from("/workspace/a.md"),
                at_millis: base,
            });
            entries.push(AttributionEntry {
                source: "Codex".to_string(),
                path: PathBuf::from("/workspace/other.md"),
                at_millis: base + 1_000,
            });
        }

        let much_later = base + ATTRIBUTION_MATCH_WINDOW_MS + 1;
        assert_eq!(
            ledger.take_match(Path::new("/workspace/a.md"), much_later),
            None,
            "stale entry must not match"
        );
        assert_eq!(
            ledger.take_match(Path::new("/workspace/b.md"), base + 2_000),
            None,
            "foreign path must not match"
        );
        assert_eq!(
            ledger.take_match(Path::new("/workspace/other.md"), base + 2_000),
            Some("Codex".to_string())
        );
    }
}
