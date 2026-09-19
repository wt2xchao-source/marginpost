use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

pub type CoreResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Heading,
    Paragraph,
    List,
    Quote,
    Code,
    Table,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffChangeType {
    Added,
    Deleted,
    Rewritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSegmentKind {
    Equal,
    Added,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSegment {
    pub kind: DiffSegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffChange {
    pub id: String,
    pub sequence: usize,
    pub block_type: BlockType,
    pub change_type: DiffChangeType,
    pub old_start: Option<usize>,
    pub old_end: Option<usize>,
    pub new_start: Option<usize>,
    pub new_end: Option<usize>,
    pub old_text: String,
    pub new_text: String,
    pub old_segments: Vec<DiffSegment>,
    pub new_segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone)]
pub struct VersionInput {
    pub root_path: String,
    pub relative_path: String,
    pub content: String,
    pub content_hash: String,
    pub encoding: String,
    pub version_type: String,
    pub created_at: u64,
    pub source_type: String,
    pub source: Option<String>,
    pub agent: Option<String>,
    pub reason: Option<String>,
    pub metadata_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersion {
    pub id: String,
    pub relative_path: String,
    pub content: String,
    pub content_hash: String,
    pub encoding: String,
    pub version_type: String,
    pub created_at: u64,
    pub source_type: String,
    pub source: Option<String>,
    pub agent: Option<String>,
    pub reason: Option<String>,
    pub schema_version: u32,
    pub metadata_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersionSummary {
    pub id: String,
    pub relative_path: String,
    pub content_hash: String,
    pub encoding: String,
    pub version_type: String,
    pub created_at: u64,
    pub source_type: String,
    pub source: Option<String>,
    pub agent: Option<String>,
    pub reason: Option<String>,
    pub schema_version: u32,
}

#[derive(Debug, Clone)]
pub struct PendingRestore {
    pub id: String,
    pub root_path: String,
    pub relative_path: String,
    pub before_version_id: String,
    pub target_version_id: String,
    pub target_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChangeSet {
    pub id: String,
    pub relative_path: String,
    pub base_version_id: String,
    pub base_content: String,
    pub base_hash: String,
    pub base_encoding: String,
    pub candidate_version_id: String,
    pub candidate_content: String,
    pub candidate_hash: String,
    pub candidate_encoding: String,
    pub source: Option<String>,
    pub detected_at: u64,
}

pub trait FileSource {
    fn list_markdown(&self, root: &Path) -> CoreResult<Vec<PathBuf>>;
    fn read(&self, path: &Path) -> CoreResult<String>;
    fn write(&self, path: &Path, content: &str) -> CoreResult<()>;
}

pub trait ChangeDetector {
    fn wait_for_change(&self, root: &Path, timeout: Duration) -> CoreResult<PathBuf>;
}

pub trait DiffEngine {
    fn compare(&self, base: &str, candidate: &str) -> Vec<DiffChange>;
}

pub trait VersionStore {
    fn initialize(&self) -> CoreResult<()>;
    fn create_workspace(&self, id: &str, root_path: &str) -> CoreResult<()>;
    fn create_document(&self, id: &str, workspace_id: &str, relative_path: &str) -> CoreResult<()>;
    fn record_version(
        &self,
        input: &VersionInput,
        deduplicate_latest: bool,
    ) -> CoreResult<DocumentVersion>;
    fn list_versions(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> CoreResult<Vec<DocumentVersionSummary>>;
    fn get_version(
        &self,
        root_path: &str,
        relative_path: &str,
        version_id: &str,
    ) -> CoreResult<Option<DocumentVersion>>;
    fn begin_restore(
        &self,
        root_path: &str,
        relative_path: &str,
        before_version_id: &str,
        target_version_id: &str,
        target_hash: &str,
        created_at: u64,
    ) -> CoreResult<String>;
    fn complete_restore(
        &self,
        operation_id: &str,
        restored_version_id: &str,
        completed_at: u64,
    ) -> CoreResult<()>;
    fn fail_restore(
        &self,
        operation_id: &str,
        error_code: &str,
        completed_at: u64,
    ) -> CoreResult<()>;
    fn pending_restores(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> CoreResult<Vec<PendingRestore>>;
    fn pending_change_sets(&self, root_path: &str) -> CoreResult<Vec<PendingChangeSet>>;
}

pub trait ReviewPolicy {
    fn can_apply(&self, expected_hash: &str, disk_hash: &str) -> bool;
}
