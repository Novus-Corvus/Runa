//! Parsing a note's Markdown for the things Runa indexes.
//!
//! [`parse_note`] extracts, in one pass:
//!
//! * **front matter** (the `---` block at the top), reading `tags` and `aliases`,
//! * **headings** (`# Title`),
//! * **wikilinks and embeds** (`[[Note]]`, `[[Note#Heading|alias]]`, `![[image.png]]`),
//! * **inline tags** (`#idea`, `#project/runa`).
//!
//! Fenced code blocks and `inline code` are skipped, so example syntax inside
//! them is not treated as real links or tags. Line numbers are 1-based.
//!
//! This is deliberately a small, forgiving scanner and not a full Markdown
//! parser. It never fails: text that does not match is simply ignored.
//! Front matter is read as a small YAML subset (`tags`/`aliases` as inline
//! lists, comma/space separated values, or `- item` lists); other keys are kept
//! only in [`Frontmatter::raw`]. Code spans that continue across lines are not
//! recognised.

use std::borrow::Cow;
use std::collections::HashSet;

/// The `---` block at the top of a note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    /// The text between the `---` lines, unmodified.
    pub raw: String,
    /// Values of the `tags` (or `tag`) key, without a leading `#`.
    pub tags: Vec<String>,
    /// Values of the `aliases` (or `alias`) key.
    pub aliases: Vec<String>,
}

/// A Markdown heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 (`#`) to 6 (`######`).
    pub level: u8,
    /// Heading text without the `#` markers.
    pub text: String,
    /// 1-based line number.
    pub line: usize,
}

/// A `[[wikilink]]` or `![[embed]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// The note (or file) being linked to. Empty for same-note links like `[[#Heading]]`.
    pub target: String,
    /// The part after `#`, if any.
    pub heading: Option<String>,
    /// The display text after `|`, if any.
    pub alias: Option<String>,
    /// True for `![[...]]`.
    pub embed: bool,
    /// 1-based line number.
    pub line: usize,
}

/// Everything extracted from one note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedNote {
    /// Front matter, if the note starts with a valid `---` block.
    pub frontmatter: Option<Frontmatter>,
    /// Headings in document order.
    pub headings: Vec<Heading>,
    /// Links and embeds in document order.
    pub links: Vec<WikiLink>,
    /// Front-matter tags followed by inline tags, de-duplicated ignoring case.
    pub tags: Vec<String>,
}

/// Parses a note's text. Never fails and never panics.
pub fn parse_note(text: &str) -> ParsedNote {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let (frontmatter, body_start) = parse_frontmatter(&lines);

    let mut note = ParsedNote::default();
    let mut tags: Vec<String> = frontmatter
        .as_ref()
        .map(|f| f.tags.clone())
        .unwrap_or_default();
    note.frontmatter = frontmatter;

    let mut fence: Option<(char, usize)> = None;
    for (index, line) in lines.iter().enumerate().skip(body_start) {
        let line_no = index + 1;

        if let Some((fence_char, fence_len)) = fence {
            if let Some((c, n, rest)) = fence_run(line) {
                if c == fence_char && n >= fence_len && rest.trim().is_empty() {
                    fence = None;
                }
            }
            continue;
        }
        if let Some((c, n, rest)) = fence_run(line) {
            if !(c == '`' && rest.contains('`')) {
                fence = Some((c, n));
                continue;
            }
        }

        if let Some((level, text)) = parse_heading(line) {
            note.headings.push(Heading {
                level,
                text,
                line: line_no,
            });
        }

        let visible = mask_inline_code(line);
        collect_links(&visible, line_no, &mut note.links);
        collect_tags(&visible, &mut tags);
    }

    note.tags = dedupe_ignoring_case(tags);
    note
}

fn parse_frontmatter(lines: &[&str]) -> (Option<Frontmatter>, usize) {
    match lines.first() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return (None, 0),
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| matches!(line.trim_end(), "---" | "..."))
        .map(|(index, _)| index);
    let Some(end) = end else {
        return (None, 0);
    };
    let block = lines.get(1..end).unwrap_or(&[]);

    #[derive(Clone, Copy, PartialEq)]
    enum Key {
        Tags,
        Aliases,
    }
    let mut tags = Vec::new();
    let mut aliases = Vec::new();
    let mut current: Option<Key> = None;

    for line in block {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let (Some(key), Some(item)) = (current, trimmed.strip_prefix('-')) {
            let value = clean_value(item, key == Key::Tags);
            if !value.is_empty() {
                match key {
                    Key::Tags => tags.push(value),
                    Key::Aliases => aliases.push(value),
                }
            }
            continue;
        }
        let starts_indented = line.starts_with(char::is_whitespace);
        match line.split_once(':') {
            Some((key, value)) if !starts_indented => {
                current = match key.trim().to_ascii_lowercase().as_str() {
                    "tags" | "tag" => Some(Key::Tags),
                    "aliases" | "alias" => Some(Key::Aliases),
                    _ => None,
                };
                let value = value.trim();
                if let (Some(key), false) = (current, value.is_empty()) {
                    let list = split_inline_list(value, key == Key::Tags);
                    match key {
                        Key::Tags => tags.extend(list),
                        Key::Aliases => aliases.extend(list),
                    }
                    current = None;
                }
            }
            _ => {
                if !starts_indented {
                    current = None;
                }
            }
        }
    }

    (
        Some(Frontmatter {
            raw: block.join("\n"),
            tags,
            aliases,
        }),
        end + 1,
    )
}

fn split_inline_list(value: &str, split_on_whitespace: bool) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    let pieces: Vec<&str> = if split_on_whitespace {
        inner
            .split(|c: char| c == ',' || c.is_whitespace())
            .collect()
    } else {
        inner.split(',').collect()
    };
    pieces
        .into_iter()
        .map(|piece| clean_value(piece, split_on_whitespace))
        .filter(|piece| !piece.is_empty())
        .collect()
}

fn clean_value(raw: &str, is_tag: bool) -> String {
    let mut value = raw.trim();
    value = value.trim_matches(|c| c == '"' || c == '\'').trim();
    if is_tag {
        value = value.trim_start_matches('#');
    }
    value.to_string()
}

/// Returns `(fence char, run length, text after the run)` for a line that
/// starts with three or more backticks or tildes.
fn fence_run(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    let c = trimmed.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&x| x == c).count();
    if count < 3 {
        return None;
    }
    // `c` is ASCII, so `count` chars is `count` bytes.
    let rest = trimmed.get(count..)?;
    Some((c, count, rest))
}

fn parse_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    let indent = line.len().saturating_sub(trimmed.len());
    if indent >= 4 || line.starts_with('\t') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = trimmed.get(hashes..)?;
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let body = rest.trim();
    let without_closing = body.trim_end_matches('#');
    let text = if without_closing.len() != body.len()
        && (without_closing.is_empty() || without_closing.ends_with(char::is_whitespace))
    {
        without_closing.trim_end()
    } else {
        body
    };
    Some((u8::try_from(hashes).ok()?, text.to_string()))
}

/// Replaces `inline code` (including the backticks) with spaces.
fn mask_inline_code(line: &str) -> Cow<'_, str> {
    if !line.contains('`') {
        return Cow::Borrowed(line);
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if c != '`' {
            out.push(c);
            i += 1;
            continue;
        }
        let run = run_length(&chars, i);
        match find_closing_run(&chars, i + run, run) {
            Some(end) => {
                for _ in i..end {
                    out.push(' ');
                }
                i = end;
            }
            None => {
                for _ in 0..run {
                    out.push('`');
                }
                i += run;
            }
        }
    }
    Cow::Owned(out)
}

fn run_length(chars: &[char], start: usize) -> usize {
    chars.iter().skip(start).take_while(|&&c| c == '`').count()
}

/// Finds a backtick run of exactly `len` at or after `from`; returns the index
/// just past it.
fn find_closing_run(chars: &[char], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars.get(j) == Some(&'`') {
            let run = run_length(chars, j);
            if run == len {
                return Some(j + run);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

fn collect_links(line: &str, line_no: usize, out: &mut Vec<WikiLink>) {
    let mut from = 0usize;
    while let Some(rel_open) = line.get(from..).and_then(|rest| rest.find("[[")) {
        let open = from + rel_open;
        let inner_start = open + 2;
        let Some(rel_close) = line.get(inner_start..).and_then(|rest| rest.find("]]")) else {
            return;
        };
        let close = inner_start + rel_close;
        let inner = line.get(inner_start..close).unwrap_or("");
        if inner.contains('[') || inner.contains(']') {
            // Not a valid link. A link opens at the *last* `[[` before its
            // `]]`, so resume there (or after this `]]` if there is none).
            // Always moving forward keeps this linear on hostile input.
            // `[` is one byte, so every offset here is a char boundary.
            from = match line.get(open + 1..close).and_then(|s| s.rfind("[[")) {
                Some(p) => open + 1 + p,
                None => close + 2,
            };
            continue;
        }
        let embed = line.get(..open).is_some_and(|before| before.ends_with('!'));
        if let Some(link) = parse_link_inner(inner, embed, line_no) {
            out.push(link);
        }
        from = close + 2;
    }
}

fn parse_link_inner(inner: &str, embed: bool, line: usize) -> Option<WikiLink> {
    let (target_part, alias) = match inner.split_once('|') {
        Some((target, alias)) => (target, Some(alias.trim().to_string())),
        None => (inner, None),
    };
    let (target, heading) = match target_part.split_once('#') {
        Some((target, heading)) => (target.trim(), Some(heading.trim().to_string())),
        None => (target_part.trim(), None),
    };
    if target.is_empty() && heading.as_deref().map_or(true, str::is_empty) {
        return None;
    }
    Some(WikiLink {
        target: target.to_string(),
        heading: heading.filter(|h| !h.is_empty()),
        alias: alias.filter(|a| !a.is_empty()),
        embed,
        line,
    })
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

fn collect_tags(line: &str, out: &mut Vec<String>) {
    let mut previous: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && previous.map_or(true, char::is_whitespace) {
            let mut tag = String::new();
            while let Some(&next) = chars.peek() {
                if is_tag_char(next) {
                    tag.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            let tag = tag.trim_end_matches('/').to_string();
            let has_non_digit = tag.chars().any(|ch| !ch.is_ascii_digit() && ch != '/');
            previous = tag.chars().last().or(Some('#'));
            if has_non_digit {
                out.push(tag);
            }
        } else {
            previous = Some(c);
        }
    }
}

fn dedupe_ignoring_case(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(note: &ParsedNote) -> Vec<&str> {
        note.links.iter().map(|l| l.target.as_str()).collect()
    }

    #[test]
    fn finds_basic_wikilinks_with_heading_and_alias() {
        let note = parse_note("See [[Odin]] and [[Thor#Hammer|the hammer]] and ![[raven.png]].");
        assert_eq!(targets(&note), vec!["Odin", "Thor", "raven.png"]);
        assert_eq!(note.links[1].heading.as_deref(), Some("Hammer"));
        assert_eq!(note.links[1].alias.as_deref(), Some("the hammer"));
        assert!(!note.links[0].embed);
        assert!(note.links[2].embed);
    }

    #[test]
    fn same_note_heading_link_has_empty_target() {
        let note = parse_note("Jump to [[#Origins]].");
        assert_eq!(note.links[0].target, "");
        assert_eq!(note.links[0].heading.as_deref(), Some("Origins"));
    }

    #[test]
    fn ignores_empty_and_broken_links() {
        let note = parse_note("[[]] [[ ]] [[unclosed and [[a [[b]] x]] ok [[Fine]]");
        assert_eq!(targets(&note), vec!["b", "Fine"]);
    }

    #[test]
    fn skips_code_blocks_and_inline_code() {
        let text = "```\n[[InFence]] #infence\n```\nreal [[Outside]] `[[InInline]] #inline` #real\n~~~~\n[[Tilde]]\n~~~~\n``a ` [[DoubleTick]] ``\nafter [[Last]]";
        let note = parse_note(text);
        assert_eq!(targets(&note), vec!["Outside", "Last"]);
        assert_eq!(note.tags, vec!["real".to_string()]);
    }

    #[test]
    fn unclosed_fence_swallows_the_rest_without_panicking() {
        let note = parse_note("before [[A]]\n```\n[[B]]\nstill code");
        assert_eq!(targets(&note), vec!["A"]);
    }

    #[test]
    fn unmatched_backtick_is_literal() {
        let note = parse_note("a ` [[Link]]");
        assert_eq!(targets(&note), vec!["Link"]);
    }

    #[test]
    fn headings_levels_and_closing_hashes() {
        let note = parse_note(
            "# One\n## Two ##\n####### seven\n#NotHeading\n    # indented\n### C# ###\n# C#",
        );
        let found: Vec<_> = note
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.line))
            .collect();
        assert_eq!(
            found,
            vec![(1, "One", 1), (2, "Two", 2), (3, "C#", 6), (1, "C#", 7)]
        );
    }

    #[test]
    fn inline_tags_follow_the_rules() {
        let note = parse_note(
            "#idea and #project/runa and #2024 and #123/4 and a#notatag and http://x.com/#frag and #tag-with_chars. Also #Ünïcode",
        );
        assert_eq!(
            note.tags,
            vec!["idea", "project/runa", "tag-with_chars", "Ünïcode"]
        );
    }

    #[test]
    fn headings_are_not_tags() {
        let note = parse_note("# Title\n## Sub");
        assert!(note.tags.is_empty());
    }

    #[test]
    fn frontmatter_inline_and_block_lists() {
        let text = "---\ntitle: Odin\ntags: [god, \"#norse\"]\naliases:\n  - Woden\n  - 'Allfather'\ntag: extra one\n---\nBody #inline [[Link]]";
        let note = parse_note(text);
        let fm = note.frontmatter.as_ref().unwrap();
        assert_eq!(fm.aliases, vec!["Woden", "Allfather"]);
        assert_eq!(fm.tags, vec!["god", "norse", "extra", "one"]);
        assert!(fm.raw.contains("title: Odin"));
        assert_eq!(note.tags, vec!["god", "norse", "extra", "one", "inline"]);
        assert_eq!(note.links[0].line, 9);
    }

    #[test]
    fn unterminated_or_missing_frontmatter_is_not_frontmatter() {
        assert!(parse_note("---\ntitle: x\nno end").frontmatter.is_none());
        assert!(parse_note("text\n---\ntags: a\n---").frontmatter.is_none());
    }

    #[test]
    fn frontmatter_block_lists_without_indent() {
        let note = parse_note("---\ntags:\n- a\n- b\nother: 1\n- c\n---\n");
        assert_eq!(note.frontmatter.unwrap().tags, vec!["a", "b"]);
    }

    #[test]
    fn crlf_and_bom_are_handled() {
        let note = parse_note("\u{feff}---\r\ntags: x\r\n---\r\n# H\r\n[[L]]\r\n");
        assert_eq!(note.tags, vec!["x"]);
        assert_eq!(note.headings[0].text, "H");
        assert_eq!(targets(&note), vec!["L"]);
    }

    #[test]
    fn tags_are_deduplicated_ignoring_case_keeping_first() {
        let note = parse_note("#Rune #rune #RUNE #other");
        assert_eq!(note.tags, vec!["Rune", "other"]);
    }

    #[test]
    fn hostile_input_never_panics() {
        let inputs = [
            "",
            "[[",
            "]]",
            "[[[[[[",
            "]]]]",
            "![[",
            "`",
            "``",
            "```",
            "#",
            "##########",
            "[[a|b|c#d#e]]",
            "💥[[💥]]💥#💥",
            "---",
            "---\n---",
            "\u{feff}",
            "\0\0\0",
            "[[a]]]]]][[[[b]]",
        ];
        for input in inputs {
            let _ = parse_note(input);
        }
        let long = "[[".repeat(50_000) + &"`".repeat(10_000) + &"#a ".repeat(10_000);
        let _ = parse_note(&long);
    }
}
