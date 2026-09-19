use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::change_monitor::ChangeMonitorState;
use crate::history::HistoryState;

const IGNORED_DIRECTORIES: [&str; 4] = [".git", "node_modules", "target", "dist"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl CommandError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntry {
    pub(crate) relative_path: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDocument {
    pub(crate) relative_path: String,
    pub(crate) content: String,
    pub(crate) content_hash: String,
    pub(crate) encoding: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub(crate) content_hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DroppedWorkspaceSelection {
    pub(crate) root_path: String,
    pub(crate) selected_relative_path: Option<String>,
}

fn io_error(code: &str, context: &str, error: std::io::Error) -> CommandError {
    CommandError::new(code, format!("{context}: {error}"))
}

pub(crate) fn canonical_workspace(root_path: &str) -> Result<PathBuf, CommandError> {
    let root = fs::canonicalize(root_path)
        .map_err(|error| io_error("WORKSPACE_UNAVAILABLE", "Unable to open workspace", error))?;

    if !root.is_dir() {
        return Err(CommandError::new(
            "WORKSPACE_NOT_DIRECTORY",
            "The selected workspace is not a folder.",
        ));
    }

    Ok(root)
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

fn collect_markdown(
    root: &Path,
    directory: &Path,
    documents: &mut Vec<DocumentEntry>,
) -> Result<(), CommandError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| io_error("WORKSPACE_READ_FAILED", "Unable to read workspace", error))?;

    for entry in entries {
        let entry = entry
            .map_err(|error| io_error("WORKSPACE_READ_FAILED", "Unable to read entry", error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("WORKSPACE_READ_FAILED", "Unable to inspect entry", error))?;
        let path = entry.path();

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            let name = entry.file_name();
            if !IGNORED_DIRECTORIES
                .iter()
                .any(|ignored| name.eq_ignore_ascii_case(ignored))
            {
                collect_markdown(root, &path, documents)?;
            }
            continue;
        }

        if !file_type.is_file() || !is_markdown(&path) {
            continue;
        }

        let relative = path.strip_prefix(root).map_err(|_| {
            CommandError::new(
                "PATH_OUTSIDE_WORKSPACE",
                "A file resolved outside the selected workspace.",
            )
        })?;

        documents.push(DocumentEntry {
            relative_path: relative.to_string_lossy().replace('\\', "/"),
            name: entry.file_name().to_string_lossy().into_owned(),
        });
    }

    Ok(())
}

fn resolve_existing_markdown(
    root_path: &str,
    relative_path: &str,
) -> Result<PathBuf, CommandError> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(CommandError::new(
            "INVALID_PATH",
            "The requested file path is not valid for this workspace.",
        ));
    }

    let root = canonical_workspace(root_path)?;
    let path = fs::canonicalize(root.join(relative)).map_err(|error| {
        io_error(
            "FILE_UNAVAILABLE",
            "Unable to open the requested Markdown file",
            error,
        )
    })?;

    if !path.starts_with(&root) {
        return Err(CommandError::new(
            "PATH_OUTSIDE_WORKSPACE",
            "The requested file is outside the selected workspace.",
        ));
    }

    if !path.is_file() || !is_markdown(&path) {
        return Err(CommandError::new(
            "UNSUPPORTED_FILE",
            "Only existing .md and .markdown files can be opened.",
        ));
    }

    Ok(path)
}

pub(crate) fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn decode_markdown(bytes: &[u8]) -> Result<(String, &'static str), CommandError> {
    let (content_bytes, encoding) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(content) => (content, "utf-8-bom"),
        None => (bytes, "utf-8"),
    };
    let content = std::str::from_utf8(content_bytes).map_err(|_| {
        CommandError::new(
            "INVALID_UTF8",
            "This file is not valid UTF-8 and cannot be edited safely.",
        )
    })?;

    Ok((content.to_string(), encoding))
}

pub(crate) fn list_markdown_files_sync(
    root_path: &str,
) -> Result<Vec<DocumentEntry>, CommandError> {
    let root = canonical_workspace(root_path)?;
    let mut documents = Vec::new();
    collect_markdown(&root, &root, &mut documents)?;
    documents.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(documents)
}

pub(crate) fn read_markdown_file_sync(
    root_path: &str,
    relative_path: &str,
) -> Result<OpenedDocument, CommandError> {
    let path = resolve_existing_markdown(root_path, relative_path)?;
    let bytes = fs::read(&path)
        .map_err(|error| io_error("FILE_READ_FAILED", "Unable to read Markdown file", error))?;
    let (content, encoding) = decode_markdown(&bytes)?;

    Ok(OpenedDocument {
        relative_path: relative_path.to_string(),
        content,
        content_hash: content_hash(&bytes),
        encoding: encoding.to_string(),
    })
}

pub(crate) fn inspect_dropped_path_sync(
    dropped_path: &str,
) -> Result<DroppedWorkspaceSelection, CommandError> {
    let path = fs::canonicalize(dropped_path).map_err(|error| {
        io_error(
            "DROPPED_PATH_UNAVAILABLE",
            "Unable to open the dropped item",
            error,
        )
    })?;

    if path.is_dir() {
        return Ok(DroppedWorkspaceSelection {
            root_path: path.to_string_lossy().into_owned(),
            selected_relative_path: None,
        });
    }

    if !path.is_file() || !is_markdown(&path) {
        return Err(CommandError::new(
            "UNSUPPORTED_DROP",
            "Drop a folder or one .md or .markdown file.",
        ));
    }

    let parent = path.parent().ok_or_else(|| {
        CommandError::new(
            "DROPPED_PATH_UNAVAILABLE",
            "The dropped Markdown file has no parent folder.",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        CommandError::new(
            "DROPPED_PATH_UNAVAILABLE",
            "The dropped Markdown file has no file name.",
        )
    })?;

    if let Some(repo_root) = find_git_repository_root(parent) {
        if let Ok(relative) = path.strip_prefix(&repo_root) {
            return Ok(DroppedWorkspaceSelection {
                root_path: repo_root.to_string_lossy().into_owned(),
                selected_relative_path: Some(relative.to_string_lossy().replace('\\', "/")),
            });
        }
    }

    Ok(DroppedWorkspaceSelection {
        root_path: parent.to_string_lossy().into_owned(),
        selected_relative_path: Some(file_name.to_string_lossy().into_owned()),
    })
}

fn find_git_repository_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(directory) = current {
        if directory.join(".git").exists() {
            return Some(directory.to_path_buf());
        }
        current = directory.parent();
    }
    None
}

pub(crate) fn save_markdown_file_sync(
    root_path: &str,
    relative_path: &str,
    content: &str,
    expected_hash: &str,
    encoding: &str,
) -> Result<SaveResult, CommandError> {
    let path = resolve_existing_markdown(root_path, relative_path)?;
    let current_bytes = fs::read(&path)
        .map_err(|error| io_error("FILE_READ_FAILED", "Unable to verify Markdown file", error))?;

    if content_hash(&current_bytes) != expected_hash {
        return Err(CommandError::new(
            "FILE_CHANGED",
            "The file changed on disk. Your draft was kept and was not overwritten.",
        ));
    }

    let bytes = encode_markdown(content, encoding)?;

    let parent = path.parent().ok_or_else(|| {
        CommandError::new("INVALID_PATH", "The Markdown file has no parent folder.")
    })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = parent.join(format!(".amr-save-{}-{timestamp}.tmp", std::process::id()));

    fs::write(&temp_path, &bytes)
        .map_err(|error| io_error("SAVE_FAILED", "Unable to write the temporary file", error))?;
    if let Err(error) = fs::rename(&temp_path, &path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(
            "SAVE_FAILED",
            "Unable to replace the Markdown file",
            error,
        ));
    }

    Ok(SaveResult {
        content_hash: content_hash(&bytes),
    })
}

pub(crate) fn encode_markdown(content: &str, encoding: &str) -> Result<Vec<u8>, CommandError> {
    let mut bytes = Vec::with_capacity(content.len() + 3);
    match encoding {
        "utf-8" => {}
        "utf-8-bom" => bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
        _ => {
            return Err(CommandError::new(
                "UNSUPPORTED_ENCODING",
                "The file encoding is not supported for saving.",
            ));
        }
    }
    bytes.extend_from_slice(content.as_bytes());
    Ok(bytes)
}

pub(crate) async fn run_blocking<T, F>(operation: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError::new("INTERNAL_ERROR", error.to_string()))?
}

#[tauri::command]
pub async fn list_markdown_files(root_path: String) -> Result<Vec<DocumentEntry>, CommandError> {
    run_blocking(move || list_markdown_files_sync(&root_path)).await
}

#[tauri::command]
pub async fn inspect_dropped_path(
    dropped_path: String,
) -> Result<DroppedWorkspaceSelection, CommandError> {
    run_blocking(move || inspect_dropped_path_sync(&dropped_path)).await
}

#[tauri::command]
pub async fn read_markdown_file(
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
) -> Result<OpenedDocument, CommandError> {
    let read_root = root_path.clone();
    let opened = run_blocking(move || read_markdown_file_sync(&read_root, &relative_path)).await?;
    history.capture_snapshot(&root_path, &opened, "snapshot", "filesystem")?;
    Ok(opened)
}

#[tauri::command]
pub async fn save_markdown_file(
    monitor: State<'_, ChangeMonitorState>,
    history: State<'_, HistoryState>,
    root_path: String,
    relative_path: String,
    content: String,
    expected_hash: String,
    encoding: String,
) -> Result<SaveResult, CommandError> {
    let before = {
        let read_root = root_path.clone();
        let read_path = relative_path.clone();
        run_blocking(move || read_markdown_file_sync(&read_root, &read_path)).await?
    };
    history.capture_snapshot(&root_path, &before, "pre_edit", "editor")?;
    let planned_bytes = encode_markdown(&content, &encoding)?;
    let planned_hash = content_hash(&planned_bytes);
    monitor.prepare_internal_save(&relative_path, &planned_hash);

    let monitor_relative_path = relative_path.clone();
    let monitor_content = content.clone();
    let save_root = root_path.clone();
    let save_encoding = encoding.clone();
    let result = run_blocking(move || {
        save_markdown_file_sync(
            &save_root,
            &relative_path,
            &content,
            &expected_hash,
            &save_encoding,
        )
    })
    .await;

    match &result {
        Ok(saved) => {
            monitor.confirm_internal_save(
                &monitor_relative_path,
                &monitor_content,
                &saved.content_hash,
            );
            history.capture_action(
                &root_path,
                &monitor_relative_path,
                &monitor_content,
                &saved.content_hash,
                &encoding,
                "editor",
                "editor",
                None,
                "{}",
            )?;
        }
        Err(_) => monitor.cancel_internal_save(&monitor_relative_path, &planned_hash),
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_supported_markdown_and_ignores_generated_directories() {
        let directory = tempfile::tempdir().expect("temp directory");
        fs::create_dir_all(directory.path().join("notes")).expect("create notes");
        fs::create_dir_all(directory.path().join("node_modules/pkg")).expect("create generated");
        fs::write(directory.path().join("root.md"), "# Root").expect("write root");
        fs::write(directory.path().join("notes/idea.markdown"), "# Idea").expect("write nested");
        fs::write(directory.path().join("notes/ignored.txt"), "text").expect("write text");
        fs::write(
            directory.path().join("node_modules/pkg/readme.md"),
            "ignored",
        )
        .expect("write generated markdown");

        let documents =
            list_markdown_files_sync(directory.path().to_str().expect("path")).expect("list files");

        assert_eq!(
            documents
                .iter()
                .map(|document| document.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["notes/idea.markdown", "root.md"]
        );
    }

    #[test]
    fn rejects_parent_directory_paths() {
        let directory = tempfile::tempdir().expect("temp directory");
        let error =
            read_markdown_file_sync(directory.path().to_str().expect("path"), "../outside.md")
                .expect_err("path must be rejected");

        assert_eq!(error.code, "INVALID_PATH");
    }

    #[test]
    fn preserves_utf8_bom_when_saving() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.md");
        fs::write(&path, [0xEF, 0xBB, 0xBF, b'#', b' ', b'A']).expect("write file");

        let opened = read_markdown_file_sync(directory.path().to_str().expect("path"), "note.md")
            .expect("open file");
        assert_eq!(opened.encoding, "utf-8-bom");
        assert_eq!(opened.content, "# A");

        save_markdown_file_sync(
            directory.path().to_str().expect("path"),
            "note.md",
            "# B",
            &opened.content_hash,
            &opened.encoding,
        )
        .expect("save file");

        assert_eq!(
            fs::read(path).expect("read saved file"),
            [0xEF, 0xBB, 0xBF, b'#', b' ', b'B']
        );
    }

    #[test]
    fn rejects_invalid_utf8() {
        let directory = tempfile::tempdir().expect("temp directory");
        fs::write(directory.path().join("note.md"), [0xFF, 0xFE]).expect("write invalid file");

        let error = read_markdown_file_sync(directory.path().to_str().expect("path"), "note.md")
            .expect_err("encoding must be rejected");

        assert_eq!(error.code, "INVALID_UTF8");
    }

    #[test]
    fn refuses_to_overwrite_a_file_that_changed_on_disk() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.md");
        fs::write(&path, "# Base").expect("write file");
        let opened = read_markdown_file_sync(directory.path().to_str().expect("path"), "note.md")
            .expect("open file");
        fs::write(&path, "# External").expect("external change");

        let error = save_markdown_file_sync(
            directory.path().to_str().expect("path"),
            "note.md",
            "# Draft",
            &opened.content_hash,
            &opened.encoding,
        )
        .expect_err("conflict must be rejected");

        assert_eq!(error.code, "FILE_CHANGED");
        assert_eq!(fs::read_to_string(path).expect("read file"), "# External");
    }

    #[test]
    fn accepts_a_dropped_folder_as_a_workspace() {
        let directory = tempfile::tempdir().expect("temp directory");

        let selection =
            inspect_dropped_path_sync(directory.path().to_str().expect("path")).expect("inspect");

        assert_eq!(
            selection.root_path,
            directory
                .path()
                .canonicalize()
                .expect("canonical")
                .to_string_lossy()
        );
        assert_eq!(selection.selected_relative_path, None);
    }

    #[test]
    fn accepts_a_dropped_markdown_file_and_selects_it() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.md");
        fs::write(&path, "# Note").expect("write");

        let selection = inspect_dropped_path_sync(path.to_str().expect("path")).expect("inspect");

        assert_eq!(
            selection.root_path,
            directory
                .path()
                .canonicalize()
                .expect("canonical")
                .to_string_lossy()
        );
        assert_eq!(selection.selected_relative_path.as_deref(), Some("note.md"));
    }

    #[test]
    fn rejects_a_dropped_non_markdown_file() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("note.txt");
        fs::write(&path, "Note").expect("write");

        let error =
            inspect_dropped_path_sync(path.to_str().expect("path")).expect_err("must reject");

        assert_eq!(error.code, "UNSUPPORTED_DROP");
    }

    #[test]
    fn opens_the_git_repository_root_for_a_dropped_markdown_file() {
        let repository = tempfile::tempdir().expect("temp directory");
        fs::create_dir_all(repository.path().join(".git")).expect("create .git");
        fs::create_dir_all(repository.path().join("docs")).expect("create docs");
        let path = repository.path().join("docs").join("guide.md");
        fs::write(&path, "# Guide").expect("write");

        let selection = inspect_dropped_path_sync(path.to_str().expect("path")).expect("inspect");

        assert_eq!(
            selection.root_path,
            repository
                .path()
                .canonicalize()
                .expect("canonical")
                .to_string_lossy()
        );
        assert_eq!(
            selection.selected_relative_path.as_deref(),
            Some("docs/guide.md")
        );
    }

    #[test]
    fn prefers_the_nearest_git_repository_root() {
        let outer = tempfile::tempdir().expect("temp directory");
        fs::create_dir_all(outer.path().join(".git")).expect("create outer .git");
        let nested = outer.path().join("sub");
        fs::create_dir_all(nested.join(".git")).expect("create nested .git");
        fs::create_dir_all(nested.join("notes")).expect("create notes");
        let path = nested.join("notes").join("a.md");
        fs::write(&path, "# A").expect("write");

        let selection = inspect_dropped_path_sync(path.to_str().expect("path")).expect("inspect");

        assert_eq!(
            selection.root_path,
            nested.canonicalize().expect("canonical").to_string_lossy()
        );
        assert_eq!(
            selection.selected_relative_path.as_deref(),
            Some("notes/a.md")
        );
    }

    #[test]
    fn recognizes_a_worktree_style_git_file() {
        let repository = tempfile::tempdir().expect("temp directory");
        fs::write(
            repository.path().join(".git"),
            "gitdir: /somewhere/else/worktree.git\n",
        )
        .expect("create .git file");
        fs::create_dir_all(repository.path().join("docs")).expect("create docs");
        let path = repository.path().join("docs").join("guide.md");
        fs::write(&path, "# Guide").expect("write");

        let selection = inspect_dropped_path_sync(path.to_str().expect("path")).expect("inspect");

        assert_eq!(
            selection.root_path,
            repository
                .path()
                .canonicalize()
                .expect("canonical")
                .to_string_lossy()
        );
        assert_eq!(
            selection.selected_relative_path.as_deref(),
            Some("docs/guide.md")
        );
    }
}
