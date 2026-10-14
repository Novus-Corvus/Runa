# Devlog

Newest entries first. Each entry says what changed, why, and what is *not yet* verified.

---

## 0.1.0: Foundation (2026-10-14)

### Added
- Cargo workspace with two crates, `runa-core` and `runa-theme`, both free of external dependencies.
- **`runa-core`**
  - `error`: `RunaError` with summary, path, technical detail and suggestions; classifies OS errors (not found, permission, in use, disk full) into actionable messages.
  - `fsio`: atomic writes (temp file, flush, rename), `save_checked` conflict detection, read-only refusal, 64 MiB note limit, lossy-UTF-8 flag on load.
  - `vault`: path resolution that rejects `..`, illegal characters, Windows reserved names, the private `.runa` folder and symlink escapes; non-fatal scanning; create / rename (never overwrites) / trash.
  - `note`: single-pass scanner for front matter (`tags`, `aliases`), headings, wikilinks and embeds, and inline tags; skips code fences and inline code.
  - `index`: link resolution (case- and extension-insensitive, bare names or paths, closest-folder preference, real ties reported as ambiguous, aliases, attachments), backlinks, unresolved links.
  - `search`: multi-term and quoted-phrase search with title ranking.
  - `rescue`: last-resort copies to `.runa/rescue`, falling back to the system temp folder.
- **`runa-theme`**: dark and light palettes named after Obsidian's concepts; six accent presets with **Rune Red** as default; any custom hex accent; computed contrast so accent text and on-accent text always meet WCAG AA (4.5:1).
- CI workflow: `fmt`, `clippy -D warnings`, and tests on Linux, Windows and macOS.
- README (status, usage, roadmap, code map) and this Devlog.

### Decisions
- **No dependencies yet.** Front matter is parsed as a small YAML subset by hand rather than pulling in a YAML crate. Revisit when richer properties are needed (Phase 3).
- **No-panic policy is enforced by the compiler**, via workspace lints, not by convention. Test code is exempt.
- **Core before UI.** The UI toolkit is deliberately undecided; the engine does not depend on it.
- **Delete means trash.** Notes are moved to `.runa/trash`, never unlinked.
- **Symlinks are reported, not followed**, during scans, to avoid loops and escapes from the vault.
- File names follow Obsidian's restrictions (no `* " \ / < > : | ?`) so vaults stay portable across operating systems.

### Fixed during development
- The link scanner was quadratic on a long line containing many `[[` and a single `]]`. It now resumes at the last `[[` before the closing `]]` and stays linear.

### Verification
- 63 tests pass (54 in `runa-core`, 9 in `runa-theme`) on Linux with Rust 1.75.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- Confirmed the no-panic lints reject a deliberate `unwrap()` and an unchecked index.
- **Not yet run:** Windows and macOS (covered by CI once pushed). Windows-specific branches (sharing-violation and disk-full error codes, reserved names) are written but only the reserved-name logic is exercised by tests.

### Known limitations
- Search re-scans note text on every query; a persistent index is planned.
- Renaming a note does not yet update links to it in other notes.
- Leftover hidden temp files after a hard crash during a save are not yet cleaned up on startup.
- Multi-line inline code spans are not recognised by the note scanner.
- Front matter beyond `tags` and `aliases` is preserved as raw text but not interpreted.
