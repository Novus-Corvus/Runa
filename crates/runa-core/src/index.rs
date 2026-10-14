//! Resolving `[[wikilinks]]` and computing backlinks.
//!
//! Resolution follows the conventions people expect from Obsidian-style vaults:
//!
//! * Matching ignores case and the `.md` extension.
//! * A link can be a bare name (`[[Odin]]`) or a path (`[[Gods/Odin]]`).
//! * If several notes match a bare name, the one closest to the linking note
//!   (most shared folders) wins; if that is still a tie, the choice is reported
//!   as [`Resolution::Ambiguous`] so the UI can tell the person.
//! * Notes are matched before attachments, and attachments before aliases
//!   declared in front matter.
//!
//! Everything is built from lookup tables, so resolving a link does not scan
//! the whole vault.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::note::ParsedNote;

/// A parsed note together with its vault-relative path.
#[derive(Debug, Clone)]
pub struct IndexedNote {
    /// Vault-relative path with `/` separators, e.g. `Gods/Odin.md`.
    pub path: String,
    /// The note's parsed contents.
    pub note: ParsedNote,
}

/// What a link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one best note.
    Found(String),
    /// Several equally good notes; `chosen` is used for backlinks.
    Ambiguous {
        /// The note picked deterministically (alphabetically first).
        chosen: String,
        /// The other equally good candidates.
        others: Vec<String>,
    },
    /// A non-note file such as an image.
    Attachment(String),
    /// Nothing matches.
    Missing,
}

/// A note that links to another note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    /// Path of the linking note.
    pub from: String,
    /// 1-based line of the link in the linking note.
    pub line: usize,
}

/// A link whose target does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedLink {
    /// Path of the linking note.
    pub from: String,
    /// The target text as written.
    pub target: String,
    /// 1-based line of the link.
    pub line: usize,
}

/// Lookup tables and link results for a whole vault.
#[derive(Debug, Default)]
pub struct LinkIndex {
    /// Lower-cased note path without `.md`, parallel to `paths`.
    keys: Vec<String>,
    paths: Vec<String>,
    by_stem: HashMap<String, Vec<usize>>,
    aliases: HashMap<String, Vec<usize>>,
    files_lower: Vec<String>,
    files: Vec<String>,
    files_by_name: HashMap<String, Vec<usize>>,
    backlinks: HashMap<String, Vec<Backlink>>,
    unresolved: Vec<UnresolvedLink>,
}

impl LinkIndex {
    /// Builds the index from parsed notes and the vault's attachment paths.
    pub fn build(notes: &[IndexedNote], attachments: &[String]) -> Self {
        let mut index = Self::default();

        for (i, indexed) in notes.iter().enumerate() {
            let key = normalize(&indexed.path);
            let stem = last_segment(&key).to_string();
            index.keys.push(key);
            index.paths.push(indexed.path.clone());
            index.by_stem.entry(stem).or_default().push(i);
            if let Some(front) = &indexed.note.frontmatter {
                for alias in &front.aliases {
                    index
                        .aliases
                        .entry(alias.trim().to_lowercase())
                        .or_default()
                        .push(i);
                }
            }
        }
        for (i, file) in attachments.iter().enumerate() {
            let lower = file.to_lowercase();
            index
                .files_by_name
                .entry(last_segment(&lower).to_string())
                .or_default()
                .push(i);
            index.files_lower.push(lower);
            index.files.push(file.clone());
        }

        let mut backlinks: HashMap<String, Vec<Backlink>> = HashMap::new();
        let mut unresolved = Vec::new();
        for indexed in notes {
            for link in &indexed.note.links {
                if link.target.is_empty() {
                    continue; // link to a heading in the same note
                }
                match index.resolve(&link.target, &indexed.path) {
                    Resolution::Found(path) | Resolution::Ambiguous { chosen: path, .. } => {
                        backlinks.entry(path).or_default().push(Backlink {
                            from: indexed.path.clone(),
                            line: link.line,
                        });
                    }
                    Resolution::Attachment(_) => {}
                    Resolution::Missing => unresolved.push(UnresolvedLink {
                        from: indexed.path.clone(),
                        target: link.target.clone(),
                        line: link.line,
                    }),
                }
            }
        }
        index.backlinks = backlinks;
        index.unresolved = unresolved;
        index
    }

    /// Resolves a link `target` written in the note at `from`.
    pub fn resolve(&self, target: &str, from: &str) -> Resolution {
        let normalized = normalize(target);
        if normalized.is_empty() {
            return Resolution::Missing;
        }

        let stem = last_segment(&normalized);
        let note_candidates: Vec<usize> = self
            .by_stem
            .get(stem)
            .map(|indices| {
                indices
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.keys
                            .get(i)
                            .is_some_and(|key| path_matches(key, &normalized))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !note_candidates.is_empty() {
            return self.pick_note(note_candidates, from);
        }

        let lower_target = target.replace('\\', "/").trim_matches('/').to_lowercase();
        if let Some(indices) = self.files_by_name.get(last_segment(&lower_target)) {
            let mut matches: Vec<&String> = indices
                .iter()
                .filter(|&&i| {
                    self.files_lower
                        .get(i)
                        .is_some_and(|f| path_matches(f, &lower_target))
                })
                .filter_map(|&i| self.files.get(i))
                .collect();
            matches.sort();
            if let Some(first) = matches.first() {
                return Resolution::Attachment((*first).clone());
            }
        }

        if !normalized.contains('/') {
            if let Some(indices) = self.aliases.get(&normalized) {
                let candidates: Vec<usize> = indices.clone();
                if !candidates.is_empty() {
                    return self.pick_note(candidates, from);
                }
            }
        }
        Resolution::Missing
    }

    /// Notes that link to `path`, in vault order.
    pub fn backlinks_to(&self, path: &str) -> &[Backlink] {
        self.backlinks.get(path).map_or(&[], Vec::as_slice)
    }

    /// Every link whose target does not exist.
    pub fn unresolved(&self) -> &[UnresolvedLink] {
        &self.unresolved
    }

    fn pick_note(&self, mut candidates: Vec<usize>, from: &str) -> Resolution {
        let from_dir = folder_of(&from.to_lowercase()).to_string();
        let score = |i: usize| -> usize {
            self.keys
                .get(i)
                .map_or(0, |key| shared_prefix(&from_dir, folder_of(key)))
        };
        candidates.sort_by_key(|&i| {
            (
                Reverse(score(i)),
                self.keys.get(i).map_or(0, String::len),
                self.paths.get(i).cloned().unwrap_or_default(),
            )
        });
        let mut ordered = candidates
            .iter()
            .filter_map(|&i| self.paths.get(i).cloned());
        let Some(chosen) = ordered.next() else {
            return Resolution::Missing;
        };
        let top = candidates
            .first()
            .map(|&i| (score(i), self.keys.get(i).map_or(0, String::len)));
        let tied: Vec<String> = candidates
            .iter()
            .skip(1)
            .filter(|&&i| Some((score(i), self.keys.get(i).map_or(0, String::len))) == top)
            .filter_map(|&i| self.paths.get(i).cloned())
            .collect();
        if tied.is_empty() {
            Resolution::Found(chosen)
        } else {
            Resolution::Ambiguous {
                chosen,
                others: tied,
            }
        }
    }
}

/// Lower-cases, converts `\` to `/`, trims slashes, and drops a `.md` suffix.
fn normalize(path: &str) -> String {
    let lower = path.replace('\\', "/").trim_matches('/').to_lowercase();
    match lower.strip_suffix(".md") {
        Some(stripped) => stripped.to_string(),
        None => lower,
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// True if `key` equals `target` or ends with `/target`.
fn path_matches(key: &str, target: &str) -> bool {
    key.strip_suffix(target)
        .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
}

fn shared_prefix(a: &str, b: &str) -> usize {
    a.split('/')
        .zip(b.split('/'))
        .take_while(|(x, y)| x == y)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::parse_note;

    fn note(path: &str, text: &str) -> IndexedNote {
        IndexedNote {
            path: path.to_string(),
            note: parse_note(text),
        }
    }

    fn build(notes: &[IndexedNote], files: &[&str]) -> LinkIndex {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        LinkIndex::build(notes, &files)
    }

    #[test]
    fn resolves_bare_names_paths_case_and_extension() {
        let index = build(
            &[note("Gods/Odin.md", ""), note("Places/Asgard.md", "")],
            &[],
        );
        let found = |t: &str| index.resolve(t, "x.md");
        assert_eq!(found("odin"), Resolution::Found("Gods/Odin.md".into()));
        assert_eq!(found("ODIN.md"), Resolution::Found("Gods/Odin.md".into()));
        assert_eq!(found("Gods/Odin"), Resolution::Found("Gods/Odin.md".into()));
        assert_eq!(
            found("gods\\odin"),
            Resolution::Found("Gods/Odin.md".into())
        );
        assert_eq!(found("Places/Odin"), Resolution::Missing);
        assert_eq!(found("Nope"), Resolution::Missing);
        assert_eq!(found(""), Resolution::Missing);
        assert_eq!(found("dgods/odin"), Resolution::Missing);
    }

    #[test]
    fn prefers_the_closest_note_and_reports_real_ties() {
        let index = build(
            &[
                note("A/Todo.md", ""),
                note("B/Todo.md", ""),
                note("C/Todo.md", ""),
                note("A/Home.md", ""),
            ],
            &[],
        );
        assert_eq!(
            index.resolve("Todo", "A/Home.md"),
            Resolution::Found("A/Todo.md".into())
        );
        match index.resolve("Todo", "Elsewhere/Note.md") {
            Resolution::Ambiguous { chosen, others } => {
                assert_eq!(chosen, "A/Todo.md");
                assert_eq!(
                    others,
                    vec!["B/Todo.md".to_string(), "C/Todo.md".to_string()]
                );
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn aliases_resolve_after_names() {
        let index = build(
            &[
                note("Odin.md", "---\naliases: [Allfather, Woden]\n---\n"),
                note("Woden.md", ""),
            ],
            &[],
        );
        assert_eq!(
            index.resolve("allfather", "x.md"),
            Resolution::Found("Odin.md".into())
        );
        // A real note named Woden beats the alias.
        assert_eq!(
            index.resolve("Woden", "x.md"),
            Resolution::Found("Woden.md".into())
        );
    }

    #[test]
    fn attachments_are_recognised_and_not_unresolved() {
        let notes = [note(
            "Raven.md",
            "![[img/Raven.PNG]] and ![[huginn.png]] and ![[ghost.png]]",
        )];
        let index = build(&notes, &["img/raven.png", "huginn.png"]);
        assert_eq!(
            index.resolve("huginn.png", "Raven.md"),
            Resolution::Attachment("huginn.png".into())
        );
        let unresolved: Vec<_> = index
            .unresolved()
            .iter()
            .map(|u| u.target.as_str())
            .collect();
        assert_eq!(unresolved, vec!["ghost.png"]);
    }

    #[test]
    fn backlinks_and_unresolved_are_collected() {
        let notes = [
            note("Odin.md", "Rules [[Asgard]]. Needs [[Valhalla]]."),
            note("Thor.md", "\n\nSon of [[odin]] in [[Asgard]]."),
            note("Asgard.md", "Self [[#Origins]] and [[asgard]]."),
        ];
        let index = build(&notes, &[]);
        assert_eq!(
            index.backlinks_to("Odin.md"),
            &[Backlink {
                from: "Thor.md".into(),
                line: 3
            }]
        );
        let asgard: Vec<_> = index
            .backlinks_to("Asgard.md")
            .iter()
            .map(|b| b.from.as_str())
            .collect();
        assert_eq!(asgard, vec!["Odin.md", "Thor.md", "Asgard.md"]);
        assert_eq!(
            index.unresolved(),
            &[UnresolvedLink {
                from: "Odin.md".into(),
                target: "Valhalla".into(),
                line: 1
            }]
        );
        assert!(index.backlinks_to("Nobody.md").is_empty());
    }

    #[test]
    fn large_vault_builds_quickly_enough() {
        let notes: Vec<IndexedNote> = (0..5_000)
            .map(|i| {
                note(
                    &format!("f{}/n{i}.md", i % 50),
                    &format!("[[n{}]] [[n{}]]", (i + 1) % 5_000, (i * 7) % 5_000),
                )
            })
            .collect();
        let start = std::time::Instant::now();
        let index = build(&notes, &[]);
        assert!(index.unresolved().is_empty());
        assert!(start.elapsed().as_secs() < 5, "index build too slow");
    }
}
