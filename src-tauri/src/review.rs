use crate::change_monitor::{ChangeMonitorState, ChangeSetSummary};
use crate::commands::{
    content_hash, encode_markdown, read_markdown_file_sync, run_blocking, save_markdown_file_sync,
    CommandError,
};
use crate::core::{DiffChange, DiffEngine, ParagraphDiffEngine};
use crate::history::HistoryState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetReview {
    summary: ChangeSetSummary,
    changes: Vec<DiffChange>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDecision {
    change_id: String,
    decision: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    content: String,
    content_hash: String,
}

#[tauri::command]
pub fn get_change_set_review(
    state: State<'_, ChangeMonitorState>,
    id: String,
) -> Option<ChangeSetReview> {
    state.get_change(&id).map(|detail| ChangeSetReview {
        summary: detail.summary,
        changes: ParagraphDiffEngine.compare(&detail.base_content, &detail.candidate_content),
    })
}

#[tauri::command]
pub async fn resolve_change_set(
    state: State<'_, ChangeMonitorState>,
    history: State<'_, HistoryState>,
    id: String,
    decisions: Vec<ChangeDecision>,
) -> Result<ResolveResult, CommandError> {
    let (root_path, detail) = state.review_context(&id).ok_or_else(|| {
        CommandError::new(
            "CHANGE_SET_UNAVAILABLE",
            "This change set is no longer available.",
        )
    })?;
    let changes = ParagraphDiffEngine.compare(&detail.base_content, &detail.candidate_content);
    let resolved_content = resolve_markdown(
        &detail.base_content,
        &detail.candidate_content,
        &changes,
        &decisions,
    )?;
    let relative_path = detail.summary.relative_path.clone();
    let candidate_hash = detail.summary.candidate_hash.clone();
    let related_change_ids = state.change_ids_for_path(&relative_path);
    let superseded_change_ids = related_change_ids
        .iter()
        .filter(|change_id| change_id.as_str() != id)
        .cloned()
        .collect::<Vec<_>>();
    let opened = {
        let root_path = root_path.clone();
        let relative_path = relative_path.clone();
        run_blocking(move || read_markdown_file_sync(&root_path, &relative_path)).await?
    };

    ensure_candidate_unchanged(&opened.content_hash, &candidate_hash)?;
    history.capture_snapshot(&root_path, &opened, "external", "external")?;

    let planned_bytes = encode_markdown(&resolved_content, &opened.encoding)?;
    let planned_hash = content_hash(&planned_bytes);
    state.prepare_internal_save(&relative_path, &planned_hash);
    let save_root = root_path.clone();
    let save_path = relative_path.clone();
    let save_content = resolved_content.clone();
    let expected_hash = opened.content_hash.clone();
    let encoding = opened.encoding.clone();
    let result = run_blocking(move || {
        save_markdown_file_sync(
            &save_root,
            &save_path,
            &save_content,
            &expected_hash,
            &encoding,
        )
    })
    .await;

    match result {
        Ok(saved) => {
            state.confirm_internal_save(&relative_path, &resolved_content, &saved.content_hash);
            let resolution_type = if decisions
                .iter()
                .all(|decision| decision.decision == "accepted")
            {
                "review_accepted"
            } else if decisions
                .iter()
                .all(|decision| decision.decision == "rejected")
            {
                "review_rejected"
            } else {
                "review_mixed"
            };
            history.capture_action(
                &root_path,
                &relative_path,
                &resolved_content,
                &saved.content_hash,
                &opened.encoding,
                resolution_type,
                "review",
                None,
                &serde_json::json!({
                    "changeSetId": id,
                    "supersededChangeSetIds": superseded_change_ids
                })
                .to_string(),
            )?;
            state.remove_changes(&related_change_ids);
            Ok(ResolveResult {
                content: resolved_content,
                content_hash: saved.content_hash,
            })
        }
        Err(error) => {
            state.cancel_internal_save(&relative_path, &planned_hash);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn discard_change_set(
    state: State<'_, ChangeMonitorState>,
    history: State<'_, HistoryState>,
    id: String,
) -> Result<(), CommandError> {
    let (root_path, detail) = state.review_context(&id).ok_or_else(|| {
        CommandError::new(
            "CHANGE_SET_UNAVAILABLE",
            "This change set is no longer available.",
        )
    })?;
    let relative_path = detail.summary.relative_path.clone();
    let related_change_ids = state.change_ids_for_path(&relative_path);
    let superseded_change_ids = related_change_ids
        .iter()
        .filter(|change_id| change_id.as_str() != id)
        .cloned()
        .collect::<Vec<_>>();
    let metadata = serde_json::json!({
        "changeSetId": id,
        "supersededChangeSetIds": superseded_change_ids,
        "discarded": true
    })
    .to_string();

    let read_root = root_path.clone();
    let read_path = relative_path.clone();
    let disk = run_blocking(move || read_markdown_file_sync(&read_root, &read_path)).await;

    match disk {
        Ok(opened) => {
            history.capture_snapshot(&root_path, &opened, "external", "external")?;
            state.confirm_internal_save(&relative_path, &opened.content, &opened.content_hash);
            history.capture_action(
                &root_path,
                &relative_path,
                &opened.content,
                &opened.content_hash,
                &opened.encoding,
                "review_discarded",
                "review",
                detail.summary.source.clone(),
                &metadata,
            )?;
        }
        Err(_) => {
            state.forget_document(&relative_path);
            history.capture_action(
                &root_path,
                &relative_path,
                &detail.candidate_content,
                &detail.summary.candidate_hash,
                &detail.candidate_encoding,
                "review_discarded",
                "review",
                detail.summary.source.clone(),
                &metadata,
            )?;
        }
    }
    state.remove_changes(&related_change_ids);
    Ok(())
}

fn resolve_markdown(
    base: &str,
    candidate: &str,
    changes: &[DiffChange],
    decisions: &[ChangeDecision],
) -> Result<String, CommandError> {
    if changes.is_empty() {
        return Ok(candidate.to_string());
    }
    let decision_map = validate_decisions(changes, decisions)?;
    if decisions
        .iter()
        .all(|decision| decision.decision == "accepted")
    {
        return Ok(candidate.to_string());
    }
    if decisions
        .iter()
        .all(|decision| decision.decision == "rejected")
    {
        return Ok(base.to_string());
    }

    let mut output = String::new();
    let mut old_cursor = 0;
    let mut new_cursor = 0;
    let mut last_accepted = false;

    for change in changes {
        let mut old_start = change.old_start.unwrap_or(old_cursor);
        let mut new_start = change.new_start.unwrap_or(new_cursor);

        if change.old_start.is_none() {
            let new_gap = candidate
                .get(new_cursor..new_start)
                .ok_or_else(invalid_diff_plan)?;
            old_start += common_prefix_len(
                base.get(old_cursor..).ok_or_else(invalid_diff_plan)?,
                new_gap,
            );
        }
        if change.new_start.is_none() {
            let old_gap = base
                .get(old_cursor..old_start)
                .ok_or_else(invalid_diff_plan)?;
            new_start += common_prefix_len(
                candidate.get(new_cursor..).ok_or_else(invalid_diff_plan)?,
                old_gap,
            );
        }

        let old_end = change.old_end.unwrap_or(old_start);
        let new_end = change.new_end.unwrap_or(new_start);
        let accepted = decision_map.get(change.id.as_str()) == Some(&"accepted");
        if accepted {
            output.push_str(
                candidate
                    .get(new_cursor..new_start)
                    .ok_or_else(invalid_diff_plan)?,
            );
            output.push_str(
                candidate
                    .get(new_start..new_end)
                    .ok_or_else(invalid_diff_plan)?,
            );
        } else {
            output.push_str(
                base.get(old_cursor..old_start)
                    .ok_or_else(invalid_diff_plan)?,
            );
            output.push_str(base.get(old_start..old_end).ok_or_else(invalid_diff_plan)?);
        }
        old_cursor = old_end;
        new_cursor = new_end;
        last_accepted = accepted;
    }

    let old_tail = base.get(old_cursor..).ok_or_else(invalid_diff_plan)?;
    let new_tail = candidate.get(new_cursor..).ok_or_else(invalid_diff_plan)?;
    output.push_str(if old_tail == new_tail || !last_accepted {
        old_tail
    } else {
        new_tail
    });
    Ok(output)
}

fn validate_decisions<'a>(
    changes: &'a [DiffChange],
    decisions: &'a [ChangeDecision],
) -> Result<HashMap<&'a str, &'a str>, CommandError> {
    if decisions.len() != changes.len() {
        return Err(incomplete_decisions());
    }
    let valid_ids = changes
        .iter()
        .map(|change| change.id.as_str())
        .collect::<HashSet<_>>();
    let mut result = HashMap::new();
    for decision in decisions {
        if !valid_ids.contains(decision.change_id.as_str())
            || !matches!(decision.decision.as_str(), "accepted" | "rejected")
            || result
                .insert(decision.change_id.as_str(), decision.decision.as_str())
                .is_some()
        {
            return Err(incomplete_decisions());
        }
    }
    Ok(result)
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    let mut length = left
        .as_bytes()
        .iter()
        .zip(right.as_bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while length > 0 && (!left.is_char_boundary(length) || !right.is_char_boundary(length)) {
        length -= 1;
    }
    length
}

fn ensure_candidate_unchanged(disk_hash: &str, candidate_hash: &str) -> Result<(), CommandError> {
    if disk_hash == candidate_hash {
        Ok(())
    } else {
        Err(CommandError::new(
            "REVIEW_CONFLICT",
            "The file changed again during review. Nothing was overwritten.",
        ))
    }
}

fn incomplete_decisions() -> CommandError {
    CommandError::new(
        "INCOMPLETE_DECISIONS",
        "Every change must be accepted or rejected before applying the review.",
    )
}

fn invalid_diff_plan() -> CommandError {
    CommandError::new(
        "INVALID_DIFF_PLAN",
        "The review could not be reconstructed safely.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_monitor::ChangeSetDetail;

    #[test]
    fn creates_a_review_from_fixed_change_set_snapshots() {
        let detail = ChangeSetDetail {
            summary: ChangeSetSummary {
                id: "cs-1".to_string(),
                relative_path: "note.md".to_string(),
                change_type: "modified".to_string(),
                base_version_id: "base".to_string(),
                base_hash: "base-hash".to_string(),
                candidate_version_id: "candidate".to_string(),
                candidate_hash: "candidate-hash".to_string(),
                status: "pending".to_string(),
                source_type: "external".to_string(),
                source: None,
                agent: None,
                reason: None,
                detected_at: 1,
                schema_version: 1,
            },
            base_content: "# Old\n".to_string(),
            candidate_content: "# New\n".to_string(),
            base_encoding: "utf-8".to_string(),
            candidate_encoding: "utf-8".to_string(),
        };
        let result = ChangeSetReview {
            summary: detail.summary,
            changes: ParagraphDiffEngine.compare(&detail.base_content, &detail.candidate_content),
        };

        assert_eq!(result.summary.id, "cs-1");
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].old_text, "# Old");
        assert_eq!(result.changes[0].new_text, "# New");
    }

    fn decisions(changes: &[DiffChange], values: &[&str]) -> Vec<ChangeDecision> {
        changes
            .iter()
            .zip(values)
            .map(|(change, value)| ChangeDecision {
                change_id: change.id.clone(),
                decision: (*value).to_string(),
            })
            .collect()
    }

    #[test]
    fn all_accept_returns_the_exact_candidate() {
        let base = "# Old\n\nOld paragraph.\n";
        let candidate = "# New\n\nNew paragraph.\n";
        let changes = ParagraphDiffEngine.compare(base, candidate);

        let resolved = resolve_markdown(
            base,
            candidate,
            &changes,
            &decisions(&changes, &["accepted", "accepted"]),
        )
        .expect("resolve");

        assert_eq!(resolved, candidate);
    }

    #[test]
    fn all_reject_returns_the_exact_base() {
        let base = "# Old\n\nOld paragraph.\n";
        let candidate = "# New\n\nNew paragraph.\n";
        let changes = ParagraphDiffEngine.compare(base, candidate);

        let resolved = resolve_markdown(
            base,
            candidate,
            &changes,
            &decisions(&changes, &["rejected", "rejected"]),
        )
        .expect("resolve");

        assert_eq!(resolved, base);
    }

    #[test]
    fn mixed_decisions_rebuild_valid_markdown() {
        let base = "# Old\n\nOld paragraph.\n\n- keep\n";
        let candidate = "# New\n\nNew paragraph.\n\n- keep\n- add\n";
        let changes = ParagraphDiffEngine.compare(base, candidate);

        let resolved = resolve_markdown(
            base,
            candidate,
            &changes,
            &decisions(&changes, &["accepted", "rejected", "accepted"]),
        )
        .expect("resolve");

        assert_eq!(resolved, "# New\n\nOld paragraph.\n\n- keep\n- add\n");
    }

    #[test]
    fn mixed_add_and_delete_decisions_preserve_spacing() {
        let base = "# Title\n\nDelete me.\n\nKeep me.\n";
        let candidate = "# Title\n\nKeep me.\n\nAdd me.\n";
        let changes = ParagraphDiffEngine.compare(base, candidate);

        let resolved = resolve_markdown(
            base,
            candidate,
            &changes,
            &decisions(&changes, &["rejected", "accepted"]),
        )
        .expect("resolve");

        assert_eq!(resolved, "# Title\n\nDelete me.\n\nKeep me.\n\nAdd me.\n");
    }

    #[test]
    fn refuses_incomplete_or_duplicate_decisions() {
        let changes = ParagraphDiffEngine.compare("# Old\n", "# New\n");

        let missing = resolve_markdown("# Old\n", "# New\n", &changes, &[]);
        let duplicate = resolve_markdown(
            "# Old\n",
            "# New\n",
            &changes,
            &[
                ChangeDecision {
                    change_id: "change-1".to_string(),
                    decision: "accepted".to_string(),
                },
                ChangeDecision {
                    change_id: "change-1".to_string(),
                    decision: "rejected".to_string(),
                },
            ],
        );

        assert_eq!(missing.expect_err("missing").code, "INCOMPLETE_DECISIONS");
        assert_eq!(
            duplicate.expect_err("duplicate").code,
            "INCOMPLETE_DECISIONS"
        );
    }

    #[test]
    fn refuses_to_apply_when_the_disk_no_longer_matches_the_candidate() {
        assert!(ensure_candidate_unchanged("candidate", "candidate").is_ok());
        assert_eq!(
            ensure_candidate_unchanged("newer-disk", "candidate")
                .expect_err("conflict")
                .code,
            "REVIEW_CONFLICT"
        );
    }
}
