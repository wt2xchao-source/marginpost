use super::ports::{
    BlockType, DiffChange, DiffChangeType, DiffEngine, DiffSegment, DiffSegmentKind,
};
use markdown::{mdast::Node, to_mdast, ParseOptions};
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
    let Ok(tree) = to_mdast(markdown, &ParseOptions::gfm()) else {
        return (!markdown.trim().is_empty())
            .then(|| MarkdownBlock {
                block_type: BlockType::Unknown,
                start: 0,
                end: markdown.len(),
                text: markdown.trim_end_matches(['\r', '\n']).to_string(),
            })
            .into_iter()
            .collect();
    };

    let mut blocks = tree
        .children()
        .into_iter()
        .flatten()
        .filter_map(|node| {
            let position = node.position()?;
            let start = position.start.offset;
            let end = position.end.offset;
            let text = markdown.get(start..end)?.trim_end_matches(['\r', '\n']);
            (!text.is_empty()).then(|| MarkdownBlock {
                block_type: ast_block_type(node),
                start,
                end,
                text: text.to_string(),
            })
        })
        .collect::<Vec<_>>();
    for index in 0..blocks.len() {
        blocks[index].end = blocks
            .get(index + 1)
            .map(|next| next.start)
            .unwrap_or(markdown.len());
    }
    blocks
}

fn ast_block_type(node: &Node) -> BlockType {
    match node {
        Node::Heading(_) => BlockType::Heading,
        Node::Paragraph(_) => BlockType::Paragraph,
        Node::List(_) => BlockType::List,
        Node::Blockquote(_) => BlockType::Quote,
        Node::Code(_) | Node::Math(_) => BlockType::Code,
        Node::Table(_) => BlockType::Table,
        _ => BlockType::Unknown,
    }
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
    fn uses_gfm_ast_for_nested_lists_and_tables() {
        let markdown =
            "- parent\n  - child\n\n| Name | Value |\n| --- | --- |\n| nested | true |\n";
        let blocks = parse_markdown_blocks(markdown);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].block_type, BlockType::List);
        assert_eq!(blocks[1].block_type, BlockType::Table);
        assert!(blocks[0].text.contains("  - child"));
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
