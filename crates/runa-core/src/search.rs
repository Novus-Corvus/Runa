//! Text search across notes.
//!
//! A note matches when **every** search term appears in its path or text
//! (case-insensitive). Terms are separated by spaces; use double quotes for an
//! exact phrase: `"world tree" yggdrasil`. Title (file name) matches rank above
//! body matches.
//!
//! This version works on text the caller already has in memory and re-scans it
//! on every search. A persistent inverted index is on the roadmap; until then
//! the caller should cache note text rather than re-reading files.

/// A parsed search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    terms: Vec<String>,
}

/// One matching note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Vault-relative path of the note.
    pub path: String,
    /// 1-based line of the first match in the body (0 if only the title matched).
    pub line: usize,
    /// The matching line, trimmed and shortened.
    pub snippet: String,
    /// Higher is better.
    pub score: u32,
}

impl Query {
    /// Parses user input. Returns `None` if it contains no search terms.
    pub fn parse(input: &str) -> Option<Self> {
        let mut terms = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        for c in input.chars() {
            match c {
                '"' => {
                    if in_quotes {
                        push_term(&mut terms, &mut current);
                    }
                    in_quotes = !in_quotes;
                }
                c if c.is_whitespace() && !in_quotes => push_term(&mut terms, &mut current),
                c => current.push(c),
            }
        }
        push_term(&mut terms, &mut current);
        if terms.is_empty() {
            None
        } else {
            Some(Self { terms })
        }
    }

    /// The lower-cased terms, for highlighting in the UI.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

fn push_term(terms: &mut Vec<String>, current: &mut String) {
    let term = current.trim().to_lowercase();
    current.clear();
    if !term.is_empty() {
        terms.push(term);
    }
}

/// Searches `(path, text)` pairs. Returns at most `limit` hits, best first
/// (ties broken by path so results are stable).
pub fn search(notes: &[(&str, &str)], query: &Query, limit: usize) -> Vec<SearchHit> {
    let mut hits: Vec<SearchHit> = notes
        .iter()
        .filter_map(|(path, text)| score_note(path, text, query))
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    hits.truncate(limit);
    hits
}

fn score_note(path: &str, text: &str, query: &Query) -> Option<SearchHit> {
    let title = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .to_lowercase();
    let lower_path = path.to_lowercase();
    let lower_text = text.to_lowercase();

    let mut score = 0u32;
    for term in &query.terms {
        let in_title = title.contains(term.as_str());
        let body_count = lower_text.matches(term.as_str()).take(20).count() as u32;
        if !in_title && body_count == 0 && !lower_path.contains(term.as_str()) {
            return None;
        }
        if in_title {
            score += 50;
        }
        score += body_count;
    }

    let found = text.lines().enumerate().find(|(_, line)| {
        let lower = line.to_lowercase();
        query.terms.iter().any(|term| lower.contains(term.as_str()))
    });
    let (line, snippet) = match found {
        Some((index, line)) => (index + 1, shorten(line.trim())),
        None => (0, String::new()),
    };
    Some(SearchHit {
        path: path.to_string(),
        line,
        snippet,
        score,
    })
}

fn shorten(line: &str) -> String {
    const MAX_CHARS: usize = 160;
    if line.chars().count() <= MAX_CHARS {
        line.to_string()
    } else {
        let mut cut: String = line.chars().take(MAX_CHARS).collect();
        cut.push('\u{2026}');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing_handles_quotes_and_blanks() {
        assert!(Query::parse("   ").is_none());
        assert!(Query::parse("\"\"").is_none());
        assert_eq!(
            Query::parse("\"World Tree\" Yggdrasil").unwrap().terms(),
            &["world tree".to_string(), "yggdrasil".to_string()]
        );
        // An unterminated quote still yields the text typed so far.
        assert_eq!(
            Query::parse("\"half").unwrap().terms(),
            &["half".to_string()]
        );
    }

    #[test]
    fn all_terms_must_match_and_titles_rank_first() {
        let notes = [
            (
                "Gods/Odin.md",
                "Allfather of the gods.\nHe hung on the world tree.",
            ),
            ("Trees/Yggdrasil.md", "The world tree. Odin visited."),
            ("Misc.md", "unrelated"),
        ];
        let q = Query::parse("odin tree").unwrap();
        let hits = search(&notes, &q, 10);
        let paths: Vec<_> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["Gods/Odin.md", "Trees/Yggdrasil.md"]);
        assert_eq!(hits[0].line, 2);
        assert!(hits[0].snippet.contains("world tree"));
    }

    #[test]
    fn title_only_match_has_line_zero() {
        let notes = [("Odin.md", "nothing relevant")];
        let hits = search(&notes, &Query::parse("odin").unwrap(), 10);
        assert_eq!(hits[0].line, 0);
    }

    #[test]
    fn limit_and_unicode_snippets_are_safe() {
        let long = format!("{} needle {}", "é".repeat(300), "ü".repeat(300));
        let notes = [
            ("a.md", long.as_str()),
            ("b.md", "needle"),
            ("c.md", "needle"),
        ];
        let hits = search(&notes, &Query::parse("needle").unwrap(), 2);
        assert_eq!(hits.len(), 2);
        let all = search(&notes, &Query::parse("needle").unwrap(), 10);
        let snippet = &all.iter().find(|h| h.path == "a.md").unwrap().snippet;
        assert!(snippet.chars().count() <= 161);
    }
}
