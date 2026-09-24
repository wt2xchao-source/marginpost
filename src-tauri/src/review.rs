use crate::change_monitor::{ChangeMonitorState, ChangeSetSummary};
use crate::commands::{
    content_hash, create_markdown_file_sync, delete_markdown_file_sync, encode_markdown,
    ensure_markdown_absent_sync, read_markdown_file_sync, rename_markdown_file_sync, run_blocking,
    save_markdown_file_sync, CommandError,
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
    relative_path: String,
    exists: bool,
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
    file_decision: Option<String>,
) -> Result<ResolveResult, CommandError> {
    let (root_path, detail) = state.actionable_review_context(&id)?;
    let requires_file_decision = detail.summary.change_type != "modified";
    if requires_file_decision && !matches!(file_decision.as_deref(), Some("accepted" | "rejected"))
    {
        return Err(incomplete_decisions());
    }
    let changes = ParagraphDiffEngine.compare(&detail.base_content, &detail.candidate_content);
    let resolved_content = resolve_markdown(
        &detail.base_content,
        &detail.candidate_content,
        &changes,
        &decisions,
    )?;
    let relative_path = detail.summary.relative_path.clone();
    let candidate_hash = detail.summary.candidate_hash.clone();
    let previous_relative_path = detail.summary.previous_relative_path.clone();
    let mut related_change_ids = detail.summary.superseded_change_set_ids.clone();
    related_change_ids.push(id.clone());
    related_change_ids.sort();
    related_change_ids.dedup();
    let superseded_change_ids = related_change_ids
        .iter()
        .filter(|change_id| change_id.as_str() != id)
        .cloned()
        .collect::<Vec<_>>();
    let opened = if detail.candidate_exists {
        let read_root = root_path.clone();
        let relative_path = relative_path.clone();
        let opened =
            match run_blocking(move || read_markdown_file_sync(&read_root, &relative_path)).await {
                Ok(opened) => opened,
                Err(error) => {
                    state.mark_stale(&id);
                    let _ = history.mark_change_set_stale(&root_path, &detail);
                    return Err(error);
                }
            };
        if let Err(error) = ensure_candidate_unchanged(&opened.content_hash, &candidate_hash) {
            state.mark_stale(&id);
            let _ = history.mark_change_set_stale(&root_path, &detail);
            return Err(error);
        }
        history.capture_snapshot(&root_path, &opened, "external", "external")?;
        Some(opened)
    } else {
        let absent_root = root_path.clone();
        let absent_path = relative_path.clone();
        if let Err(error) =
            run_blocking(move || ensure_markdown_absent_sync(&absent_root, &absent_path)).await
        {
            state.mark_stale(&id);
            let _ = history.mark_change_set_stale(&root_path, &detail);
            return Err(error);
        }
        None
    };

    let encoding = opened
        .as_ref()
        .map(|document| document.encoding.clone())
        .unwrap_or_else(|| detail.base_encoding.clone());
    let planned_bytes = encode_markdown(&resolved_content, &encoding)?;
    let planned_hash = content_hash(&planned_bytes);
    let operation_accepted = file_decision.as_deref() != Some("rejected");
    let change_type = detail.summary.change_type.as_str();
    let result_relative_path = if change_type == "renamed" && !operation_accepted {
        previous_relative_path
            .clone()
            .ok_or_else(|| CommandError::new("INVALID_CHANGE_SET", "Rename source is missing."))?
    } else {
        relative_path.clone()
    };
    let result_exists = match change_type {
        "created" => operation_accepted,
        "deleted" => !operation_accepted,
        _ => true,
    };

    let operation_result = match (change_type, operation_accepted) {
        ("modified", _) | ("created", true) | ("renamed", true) => {
            if change_type == "renamed" {
                let previous = previous_relative_path.clone().ok_or_else(|| {
                    CommandError::new("INVALID_CHANGE_SET", "Rename source is missing.")
                })?;
                let check_root = root_path.clone();
                run_blocking(move || ensure_markdown_absent_sync(&check_root, &previous)).await?;
            }
            state.prepare_internal_save(&relative_path, &planned_hash);
            let save_root = root_path.clone();
            let save_path = relative_path.clone();
            let save_content = resolved_content.clone();
            let save_encoding = encoding.clone();
            if let Some(opened) = opened.as_ref() {
                let expected_hash = opened.content_hash.clone();
                run_blocking(move || {
                    save_markdown_file_sync(
                        &save_root,
                        &save_path,
                        &save_content,
                        &expected_hash,
                        &save_encoding,
                    )
                })
                .await
            } else {
                run_blocking(move || {
                    create_markdown_file_sync(&save_root, &save_path, &save_content, &save_encoding)
                })
                .await
            }
        }
        ("created", false) => {
            let delete_root = root_path.clone();
            let delete_path = relative_path.clone();
            let expected_hash = candidate_hash.clone();
            run_blocking(move || {
                delete_markdown_file_sync(&delete_root, &delete_path, &expected_hash)?;
                Ok(crate::commands::SaveResult {
                    content_hash: content_hash(&[]),
                })
            })
            .await
        }
        ("deleted", true) => Ok(crate::commands::SaveResult {
            content_hash: content_hash(&[]),
        }),
        ("deleted", false) => {
            state.prepare_internal_save(&relative_path, &planned_hash);
            let create_root = root_path.clone();
            let create_path = relative_path.clone();
            let create_content = resolved_content.clone();
            let create_encoding = encoding.clone();
            run_blocking(move || {
                create_markdown_file_sync(
                    &create_root,
                    &create_path,
                    &create_content,
                    &create_encoding,
                )
            })
            .await
        }
        ("renamed", false) => {
            let previous = previous_relative_path.clone().ok_or_else(|| {
                CommandError::new("INVALID_CHANGE_SET", "Rename source is missing.")
            })?;
            let move_root = root_path.clone();
            let move_from = relative_path.clone();
            let move_to = previous.clone();
            let expected_hash = candidate_hash.clone();
            let rename_hash = expected_hash.clone();
            run_blocking(move || {
                rename_markdown_file_sync(&move_root, &move_from, &move_to, &rename_hash)
            })
            .await?;
            state.prepare_internal_save(&previous, &planned_hash);
            let save_root = root_path.clone();
            let save_content = resolved_content.clone();
            let save_encoding = encoding.clone();
            run_blocking(move || {
                save_markdown_file_sync(
                    &save_root,
                    &previous,
                    &save_content,
                    &expected_hash,
                    &save_encoding,
                )
            })
            .await
        }
        _ => Err(CommandError::new(
            "INVALID_CHANGE_SET",
            "The file operation is not supported.",
        )),
    };

    match operation_result {
        Ok(saved) => {
            match (change_type, operation_accepted) {
                ("created", false) | ("deleted", true) => {
                    state.confirm_internal_delete(&relative_path)
                }
                ("renamed", true) => state.confirm_internal_rename(
                    previous_relative_path.as_deref().expect("rename source"),
                    &relative_path,
                    &resolved_content,
                    &saved.content_hash,
                    &encoding,
                ),
                ("renamed", false) => state.confirm_internal_rename(
                    &relative_path,
                    previous_relative_path.as_deref().expect("rename source"),
                    &resolved_content,
                    &saved.content_hash,
                    &encoding,
                ),
                _ => state.confirm_internal_save(
                    &relative_path,
                    &resolved_content,
                    &saved.content_hash,
                ),
            }
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
                &result_relative_path,
                &resolved_content,
                &saved.content_hash,
                &encoding,
                resolution_type,
                "review",
                None,
                &serde_json::json!({
                    "changeSetId": id,
                    "supersededChangeSetIds": superseded_change_ids,
                    "fileDecision": file_decision,
                    "exists": result_exists
                })
                .to_string(),
            )?;
            state.remove_changes(&related_change_ids);
            Ok(ResolveResult {
                content: resolved_content,
                content_hash: saved.content_hash,
                relative_path: result_relative_path,
                exists: result_exists,
            })
        }
        Err(error) => {
            state.cancel_internal_save(&relative_path, &planned_hash);
            if let Some(previous) = previous_relative_path.as_deref() {
                state.cancel_internal_save(previous, &planned_hash);
            }
            state.mark_stale(&id);
            let _ = history.mark_change_set_stale(&root_path, &detail);
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
    let (root_path, detail) = state.actionable_review_context(&id)?;
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
                previous_relative_path: None,
                change_type: "modified".to_string(),
                base_version_id: "base".to_string(),
                base_hash: "base-hash".to_string(),
                candidate_version_id: "candidate".to_string(),
                candidate_hash: "candidate-hash".to_string(),
                status: "pending".to_string(),
                superseded_by: None,
                superseded_change_set_ids: Vec::new(),
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
            base_exists: true,
            candidate_exists: true,
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
