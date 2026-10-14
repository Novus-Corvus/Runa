# Runa ᚱ

**An open-source, native, all-Rust note-taking app in the spirit of Obsidian, with a Norse-mythology soul.**

Part of the [Novus Corvus](https://github.com/Novus-Corvus) collective. Licensed GPLv3.

---

## What is this?

Runa is a Markdown note-taking app: your notes are plain `.md` files in a folder (a *vault*), linked together with `[[wikilinks]]`, searchable, and yours forever. The goal is full parity with the Obsidian experience (same layout, same workflows, same muscle memory), re-skinned with a Norse identity and written entirely in Rust with no web runtime.

## Why does it exist?

- **Open and modifiable.** The code is readable, documented, and free to change.
- **Native and fast.** No browser engine under the hood. Startup and typing should feel instant.
- **Trustworthy with your writing.** Notes are the one thing an app must never lose. Runa is built around that promise (see below).

---

## Status

> **Version 0.1.0: foundation only. There is no window yet.**

What exists today is the engine that the app will stand on, written first because it is the part that must be right. It is tested, lint-clean, and has no dependencies beyond the Rust standard library.

| Area | State |
| --- | --- |
| Vault: open, scan, create, rename, move to trash | Done, tested |
| Crash-safe saving, outside-change (conflict) detection | Done, tested |
| Markdown parsing: front matter, headings, `[[links]]`, `![[embeds]]`, `#tags` | Done, tested |
| Link resolution, backlinks, unresolved links | Done, tested |
| Search (multi-term, quoted phrases, title ranking) | Done, tested (in-memory) |
| Rescue copies of unsaveable text | Done, tested |
| Error type written for humans | Done, tested |
| Theme system: dark/light, 6 accents, custom accent, contrast guarantees | Done, tested |
| Application window, editor, panes, settings UI | **Not started** |

Everything in the table was built and tested on Linux. Windows and macOS are covered by the CI workflow (`.github/workflows/ci.yml`) but have not been run yet. Please report anything that behaves differently.

---

## The safety contract

These are the rules the code is held to, and how each is enforced.

1. **Nothing panics.** `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!` and unchecked indexing are *denied by the compiler* in non-test code (see `[workspace.lints]` in `Cargo.toml`). `unsafe` is forbidden. Hostile inputs (huge lines, unbalanced brackets, odd Unicode, invalid UTF-8) are part of the test suite.
2. **Every failure is shown, and can be acted on.** All operations return a `RunaError` carrying a plain-language summary, the file involved, the system's technical detail, and a list of things the person can try. Errors are never swallowed; partial failures (such as one unreadable folder during a scan) are *reported alongside* the successful results instead of aborting.
3. **A save never leaves a half-written note.** Text is written to a temporary file, flushed to disk, then swapped in with one rename. If anything fails, the original is untouched and the person is told their text is still safe in the editor.
4. **A save never silently overwrites someone else's edit.** If a file changed on disk (a sync tool, another editor) since it was opened, saving is refused with a *conflict* the person can resolve.
5. **Deleting is reversible.** "Delete" moves notes to `.runa/trash` inside the vault.
6. **There is always a last resort.** If saving fails, "Save a copy" writes to `.runa/rescue`, or to the system temp folder if the vault itself is the problem.
7. **Odd files are flagged, not mangled.** Files that are not valid UTF-8, are read-only, are too large, or sit behind a symbolic link are reported, never silently altered.

Known limits are documented at the top of the relevant source files rather than hidden. The main one: checking for an outside change and then replacing the file are two steps, so another program could, in principle, write in the microseconds between them.

---

## Usage

Requirements: a recent stable [Rust toolchain](https://rustup.rs) (1.75 or newer).

```bash
git clone https://github.com/Novus-Corvus/Runa.git
cd Runa

cargo test --workspace                                   # run all tests
cargo clippy --workspace --all-targets -- -D warnings    # lints, including the no-panic rules
cargo fmt --all                                          # format
```

There is no application to launch yet. Until the UI lands, the crates can be used as libraries:

```rust
use runa_core::vault::Vault;
use runa_core::{fsio, note::parse_note};

fn main() -> runa_core::Result<()> {
    let vault = Vault::open(std::path::Path::new("my-vault"))?;
    let report = vault.scan();
    for problem in &report.problems {
        eprintln!("{}", problem.report()); // show these to the person
    }
    for entry in &report.entries {
        let loaded = fsio::read_text(&vault.resolve(&entry.rel_path)?)?;
        let parsed = parse_note(&loaded.text);
        println!("{}: {} links, {} tags", entry.rel_path, parsed.links.len(), parsed.tags.len());
    }
    Ok(())
}
```

---

## Roadmap

Each phase ends with something usable. Order matters: trust first, then speed, then breadth.

**Phase 0: Foundation** (done)
Vault, safe I/O, parser, link index, search, rescue, theme.

**Phase 1: The shell**
Choose the UI toolkit (see *Open decisions*). Main window with Obsidian's layout: ribbon, file explorer, tab strip, status bar. Central error panel. Settings window with *Appearance*: dark/light, live accent switching (default Rune Red), font size. Settings persisted safely.

**Phase 2: Editing**
Source-mode editor with undo/redo, autosave through `save_checked`, conflict dialog (keep mine / reload / save a copy), lossy-encoding warning, crash-recovery of unsaved text on next launch.

**Phase 3: Navigation and knowledge**
Quick switcher, command palette, search pane, backlinks and outline panes, tag pane, rename-with-link-update, split panes, persistent search index for large vaults, file watching.

**Phase 4: Live Preview and reading view**
Obsidian-style in-place Markdown rendering, embeds, callouts, tables, code highlighting, math.

**Phase 5: Parity and beyond**
Graph view, properties editor, templates, daily notes, bookmarks, configurable hotkeys, canvas, plugin system, installers for Windows, macOS and Linux.

### Open decisions

- **UI toolkit.** The hard part of Obsidian parity is a fast, rich, editable text surface (Live Preview). Candidates: `egui`, `iced`, `Slint`, `gpui`. This choice shapes Phase 1 onward and should be made deliberately, with a small editor prototype in each finalist.
- **Plugin system.** Likely WebAssembly-based. Deferred until the core app is stable.

---

## How to navigate the code

```
Runa/
├── Cargo.toml                  Workspace definition and the no-panic lint rules
├── README.md                   You are here
├── Devlog.md                   What changed, when, and why
├── .github/workflows/ci.yml    fmt + clippy + tests on Linux, Windows, macOS
└── crates/
    ├── runa-core/              Everything except pixels. No dependencies.
    │   └── src/
    │       ├── error.rs        RunaError: summary, path, detail, suggestions
    │       ├── fsio.rs         Atomic writes, conflict detection, safe reads
    │       ├── vault.rs        Open a folder; safe paths; scan; create/rename/trash
    │       ├── note.rs         Markdown scanner: front matter, headings, links, tags
    │       ├── index.rs        Link resolution, backlinks, unresolved links
    │       ├── search.rs       Multi-term search over note text
    │       └── rescue.rs       Last-resort copies of unsaveable text
    └── runa-theme/             Colours. No dependencies, no UI toolkit.
        └── src/lib.rs          Palette, accent derivation, contrast maths
```

Where to look for...

| ...if you want to change | open |
| --- | --- |
| What an error says or suggests | `runa-core/src/error.rs` |
| How saving works | `runa-core/src/fsio.rs` |
| Which file names are allowed | `component_problem` in `runa-core/src/vault.rs` |
| What counts as a link or tag | `runa-core/src/note.rs` |
| Which note a `[[link]]` points to | `runa-core/src/index.rs` |
| The accent colours or the dark/light surfaces | `runa-theme/src/lib.rs` |

Conventions: paths inside a vault are always vault-relative with `/` separators; line numbers are 1-based; every public item has a doc comment (`missing_docs` is on); tests live at the bottom of the file they test.

---

## Principles

Runa follows the [Novus Corvus principles](https://github.com/Novus-Corvus): accessible, understandable, modifiable, simple, unobfuscated, documented. In practice: no dependencies until one is clearly worth its weight, small functions, comments that explain *why*, and no cleverness for its own sake.

## Contributing

Issues and pull requests are welcome. Before opening a PR, run the three commands under *Usage*; CI runs the same ones. Please add a line to `Devlog.md` for any user-visible or architectural change.

## License

[GNU General Public License v3.0](LICENSE)
