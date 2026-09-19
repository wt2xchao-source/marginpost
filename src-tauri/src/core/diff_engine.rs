use super::ports::{
    BlockType, DiffChange, DiffChangeType, DiffEngine, DiffSegment, DiffSegmentKind,
};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownBlock {
    block_type: BlockType,
    start: usize,
    end: usize,
    text: String,
}

pub struct ParagraphDiffEngine;

impl DiffEngine for ParagraphDiffEngine {
    fn compare(&self, base: &str, candidate: &str) -> Vec<DiffChange> {
        let old_blocks = parse_markdown_blocks(base);
        let new_blocks = parse_markdown_blocks(candidate);
        let old_keys = old_blocks.iter().map(block_key).collect::<Vec<_>>();
        let new_keys = new_blocks.iter().map(block_key).collect::<Vec<_>>();
        let old_refs = old_keys.iter().map(String::as_str).collect::<Vec<_>>();
        let new_refs = new_keys.iter().map(String::as_str).collect::<Vec<_>>();
        let diff = TextDiff::from_slices(&old_refs, &new_refs);
        let mut changes = Vec::new();
        let mut deleted = Vec::new();
        let mut inserted = Vec::new();

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => deleted.push(change.old_index().expect("deleted block index")),
                ChangeTag::Insert => {
                    inserted.push(change.new_index().expect("inserted block index"))
                }
                ChangeTag::Equal => flush_changed_blocks(
                    &mut changes,
                    &old_blocks,
                    &new_blocks,
                    &mut deleted,
                    &mut inserted,
                ),
            }
        }
        flush_changed_blocks(
            &mut changes,
            &old_blocks,
            &new_blocks,
            &mut deleted,
            &mut inserted,
        );

        for (sequence, change) in changes.iter_mut().enumerate() {
            change.sequence = sequence;
            change.id = format!("change-{}", sequence + 1);
        }
        changes
    }
}

fn block_key(block: &MarkdownBlock) -> String {
    format!("{:?}\n{}", block.block_type, block.text.trim())
}

fn flush_changed_blocks(
    changes: &mut Vec<DiffChange>,
    old_blocks: &[MarkdownBlock],
    new_blocks: &[MarkdownBlock],
    deleted: &mut Vec<usize>,
    inserted: &mut Vec<usize>,
) {
    let pair_count = deleted.len().min(inserted.len());

    for index in 0..pair_count {
        let old = &old_blocks[deleted[index]];
        let new = &new_blocks[inserted[index]];
        if old.block_type == new.block_type {
            changes.push(rewritten_change(old, new));
        } else {
            changes.push(deleted_change(old));
            changes.push(added_change(new));
        }
    }
    for old_index in deleted.iter().skip(pair_count) {
        changes.push(deleted_change(&old_blocks[*old_index]));
    }
    for new_index in inserted.iter().skip(pair_count) {
        changes.push(added_change(&new_blocks[*new_index]));
    }

    deleted.clear();
    inserted.clear();
}

fn rewritten_change(old: &MarkdownBlock, new: &MarkdownBlock) -> DiffChange {
    let (old_segments, new_segments) = sentence_diff(&old.text, &new.text);
    DiffChange {
        id: String::new(),
        sequence: 0,
        block_type: old.block_type,
        change_type: DiffChangeType::Rewritten,
        old_start: Some(old.start),
        old_end: Some(old.end),
        new_start: Some(new.start),
        new_end: Some(new.end),
        old_text: old.text.clone(),
        new_text: new.text.clone(),
        old_segments,
        new_segments,
    }
}

fn deleted_change(old: &MarkdownBlock) -> DiffChange {
    DiffChange {
        id: String::new(),
        sequence: 0,
        block_type: old.block_type,
        change_type: DiffChangeType::Deleted,
        old_start: Some(old.start),
        old_end: Some(old.end),
        new_start: None,
        new_end: None,
        old_text: old.text.clone(),
        new_text: String::new(),
        old_segments: vec![DiffSegment {
            kind: DiffSegmentKind::Deleted,
            text: old.text.clone(),
        }],
        new_segments: Vec::new(),
    }
}

fn added_change(new: &MarkdownBlock) -> DiffChange {
    DiffChange {
        id: String::new(),
        sequence: 0,
        block_type: new.block_type,
        change_type: DiffChangeType::Added,
        old_start: None,
        old_end: None,
        new_start: Some(new.start),
        new_end: Some(new.end),
        old_text: String::new(),
        new_text: new.text.clone(),
        old_segments: Vec::new(),
        new_segments: vec![DiffSegment {
            kind: DiffSegmentKind::Added,
            text: new.text.clone(),
        }],
    }
}

fn sentence_diff(old: &str, new: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    let old_sentences = split_sentences(old);
    let new_sentences = split_sentences(new);
    let old_refs = old_sentences.iter().map(String::as_str).collect::<Vec<_>>();
    let new_refs = new_sentences.iter().map(String::as_str).collect::<Vec<_>>();
    let diff = TextDiff::from_slices(&old_refs, &new_refs);
    let mut old_segments = Vec::new();
    let mut new_segments = Vec::new();

    for change in diff.iter_all_changes() {
        let text = change.value().to_string();
        match change.tag() {
            ChangeTag::Equal => {
                old_segments.push(DiffSegment {
                    kind: DiffSegmentKind::Equal,
                    text: text.clone(),
                });
                new_segments.push(DiffSegment {
                    kind: DiffSegmentKind::Equal,
                    text,
                });
            }
            ChangeTag::Delete => old_segments.push(DiffSegment {
                kind: DiffSegmentKind::Deleted,
                text,
            }),
            ChangeTag::Insert => new_segments.push(DiffSegment {
                kind: DiffSegmentKind::Added,
                text,
            }),
        }
    }
    (old_segments, new_segments)
}

fn split_sentences(text: &str) -> Vec<String> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index].1;
        let previous_character = index.checked_sub(1).and_then(|value| chars.get(value));
        let next_character = chars.get(index + 1).map(|(_, value)| *value);
        let is_decimal_dot = character == '.'
            && previous_character.is_some_and(|(_, value)| value.is_ascii_digit())
            && next_character.is_some_and(|value| value.is_ascii_digit());
        let boundary = character == '\n'
            || (!is_decimal_dot
                && matches!(character, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';'));

        if boundary {
            let mut end_index = index + 1;
            while end_index < chars.len()
                && matches!(
                    chars[end_index].1,
                    '"' | '\'' | '”' | '’' | '》' | '」' | '』' | ')' | ']' | '}' | ' '
                )
            {
                end_index += 1;
            }
            let end = chars
                .get(end_index)
                .map(|(position, _)| *position)
                .unwrap_or(text.len());
            if start < end {
                sentences.push(text[start..end].to_string());
            }
            start = end;
            index = end_index;
        } else {
            index += 1;
        }
    }

    if start < text.len() {
        sentences.push(text[start..].to_string());
    }
    if sentences.is_empty() && !text.is_empty() {
        sentences.push(text.to_string());
    }
    sentences
}

fn parse_markdown_blocks(markdown: &str) -> Vec<MarkdownBlock> {
    let lines = markdown.split_inclusive('\n').collect::<Vec<_>>();
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        line_starts.push(offset);
        offset += line.len();
    }
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        if line_body(lines[index]).trim().is_empty() {
            index += 1;
            continue;
        }

        let trimmed = line_body(lines[index]).trim_start();
        if let Some(fence) = fence_marker(trimmed) {
            let start = index;
            index += 1;
            while index < lines.len() {
                let current = line_body(lines[index]).trim_start();
                index += 1;
                if current.starts_with(fence) {
                    break;
                }
            }
            blocks.push(block(
                BlockType::Code,
                &lines[start..index],
                line_starts[start],
            ));
            continue;
        }

        if is_heading(trimmed) {
            blocks.push(block(
                BlockType::Heading,
                &lines[index..index + 1],
                line_starts[index],
            ));
            index += 1;
            continue;
        }

        if is_list_item(trimmed) {
            let start = index;
            index += 1;
            while index < lines.len() {
                let current = line_body(lines[index]);
                if current.trim().is_empty() {
                    break;
                }
                if is_list_item(current.trim_start())
                    || current.starts_with(' ')
                    || current.starts_with('\t')
                {
                    index += 1;
                } else {
                    break;
                }
            }
            blocks.push(block(
                BlockType::List,
                &lines[start..index],
                line_starts[start],
            ));
            continue;
        }

        if trimmed.starts_with('>') {
            let start = index;
            index += 1;
            while index < lines.len() && line_body(lines[index]).trim_start().starts_with('>') {
                index += 1;
            }
            blocks.push(block(
                BlockType::Quote,
                &lines[start..index],
                line_starts[start],
            ));
            continue;
        }

        if is_table_start(&lines, index) {
            let start = index;
            index += 2;
            while index < lines.len() && line_body(lines[index]).contains('|') {
                index += 1;
            }
            blocks.push(block(
                BlockType::Table,
                &lines[start..index],
                line_starts[start],
            ));
            continue;
        }

        let start = index;
        index += 1;
        while index < lines.len() {
            let current = line_body(lines[index]);
            let current_trimmed = current.trim_start();
            if current.trim().is_empty()
                || fence_marker(current_trimmed).is_some()
                || is_heading(current_trimmed)
                || is_list_item(current_trimmed)
                || current_trimmed.starts_with('>')
                || is_table_start(&lines, index)
            {
                break;
            }
            index += 1;
        }
        blocks.push(block(
            BlockType::Paragraph,
            &lines[start..index],
            line_starts[start],
        ));
    }
    for index in 0..blocks.len() {
        blocks[index].end = blocks
            .get(index + 1)
            .map(|next| next.start)
            .unwrap_or(markdown.len());
    }
    blocks
}

fn block(block_type: BlockType, lines: &[&str], start: usize) -> MarkdownBlock {
    let raw = lines.concat();
    let text = raw.trim_end_matches('\n').to_string();
    MarkdownBlock {
        block_type,
        start,
        end: start + raw.len(),
        text,
    }
}

fn line_body(line: &str) -> &str {
    let without_newline = line.strip_suffix('\n').unwrap_or(line);
    without_newline
        .strip_suffix('\r')
        .unwrap_or(without_newline)
}

fn fence_marker(line: &str) -> Option<&'static str> {
    if line.starts_with("```") {
        Some("```")
    } else if line.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn is_heading(line: &str) -> bool {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    (1..=6).contains(&hashes) && line.chars().nth(hashes).is_some_and(char::is_whitespace)
}

fn is_list_item(line: &str) -> bool {
    if ["- ", "* ", "+ "]
        .iter()
        .any(|marker| line.starts_with(marker))
    {
        return true;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && line
            .get(digits..)
            .is_some_and(|rest| rest.starts_with(". ") || rest.starts_with(") "))
}

fn is_table_start(lines: &[&str], index: usize) -> bool {
    let Some(next_line) = lines.get(index + 1) else {
        return false;
    };
    line_body(lines[index]).contains('|') && is_table_delimiter(line_body(next_line))
}

fn is_table_delimiter(line: &str) -> bool {
    let cells = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let content = cell.trim_matches(':');
            content.len() >= 3 && content.chars().all(|character| character == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_heading_and_paragraph_rewrites_as_structured_changes() {
        let changes = ParagraphDiffEngine.compare(
            "# 标题\n\n第一句不变。第二句需要修改。\n",
            "# 新标题\n\n第一句不变。第二句已经修改。\n",
        );

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].block_type, BlockType::Heading);
        assert_eq!(changes[0].change_type, DiffChangeType::Rewritten);
        assert_eq!(changes[1].block_type, BlockType::Paragraph);
        assert!(changes[1].old_segments.iter().any(|segment| {
            segment.kind == DiffSegmentKind::Equal && segment.text == "第一句不变。"
        }));
        assert!(changes[1].new_segments.iter().any(|segment| {
            segment.kind == DiffSegmentKind::Added && segment.text == "第二句已经修改。"
        }));
    }

    #[test]
    fn splits_chinese_english_and_mixed_sentences_without_character_noise() {
        let sentences = split_sentences("中文一句。English sentence. 混排 AI 很自然！最后一句");

        assert_eq!(
            sentences,
            vec![
                "中文一句。",
                "English sentence. ",
                "混排 AI 很自然！",
                "最后一句"
            ]
        );
    }

    #[test]
    fn reports_list_additions_inside_a_list_block() {
        let changes = ParagraphDiffEngine.compare("- one\n- two\n", "- one\n- two\n- three\n");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].block_type, BlockType::List);
        assert_eq!(changes[0].change_type, DiffChangeType::Rewritten);
        assert!(changes[0].new_text.contains("- three"));
    }

    #[test]
    fn classifies_independent_added_and_deleted_blocks() {
        let added = ParagraphDiffEngine.compare("# Title\n", "# Title\n\nNew paragraph.\n");
        let deleted = ParagraphDiffEngine.compare("# Title\n\nOld paragraph.\n", "# Title\n");

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].change_type, DiffChangeType::Added);
        assert_eq!(added[0].new_text, "New paragraph.");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].change_type, DiffChangeType::Deleted);
        assert_eq!(deleted[0].old_text, "Old paragraph.");
    }

    #[test]
    fn recognizes_quotes_code_and_tables_as_stable_blocks() {
        let markdown =
            "> Quote\n> More\n\n```rust\nlet value = 1;\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let blocks = parse_markdown_blocks(markdown);

        assert_eq!(
            blocks
                .iter()
                .map(|block| block.block_type)
                .collect::<Vec<_>>(),
            vec![BlockType::Quote, BlockType::Code, BlockType::Table]
        );
    }

    #[test]
    fn produces_stable_order_and_ids() {
        let base = "# Title\n\nOld paragraph.\n\n- one\n";
        let candidate = "# Better title\n\nNew paragraph.\n\n- one\n- two\n";

        let first = ParagraphDiffEngine.compare(base, candidate);
        let second = ParagraphDiffEngine.compare(base, candidate);

        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|change| change.id.as_str())
                .collect::<Vec<_>>(),
            vec!["change-1", "change-2", "change-3"]
        );
    }

    #[test]
    fn returns_no_changes_for_identical_markdown() {
        assert!(ParagraphDiffEngine
            .compare("# Same\n", "# Same\n")
            .is_empty());
    }
}
