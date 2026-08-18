# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

Collaborative rich-text editor: Rust/WASM client (Yew) synced through a Cloudflare Worker
relay (Durable Object) using Automerge CRDT. See [README.md](README.md) for architecture
and [worker/README.md](worker/README.md) for the relay/protocol details.

## Rust changes

After any change to Rust code, always run, from the `client/` directory:

```bash
cargo fmt
cargo clippy --target wasm32-unknown-unknown -- -D warnings
```

Fix any warnings clippy raises rather than silencing them, unless there's a specific reason
to allow one (and if so, scope the `#[allow(...)]` as narrowly as possible with a comment
explaining why).

## Changelog

Keep [CHANGELOG.md](CHANGELOG.md) up to date. Rules:

- **Date-based, not version-based** — this project doesn't cut releases, so entries are
  grouped by date (`## YYYY-MM-DD`), not version numbers.
- **What, not how** — each entry is one line describing the user-visible or architecturally
  significant change (e.g. "Replaced TinyMCE with ProseMirror for rich-text editing").
  Do not describe implementation details, file names, or line counts — that's what git
  history and diffs are for.
- Add an entry whenever you land a change worth remembering: new features, architecture
  changes, dependency swaps, removed functionality. Skip pure formatting/typo fixes.
