# Changelog

Notable changes to this project, newest first. Entries describe *what* changed, not *how* —
see git history for implementation details. This project doesn't cut versioned releases, so
entries are grouped by date instead.

## 2026-08-18

- Replaced TinyMCE with Quill for rich-text editing.
- View mode now renders through a read-only Quill instance instead of hand-built HTML,
  so headers and lists display correctly and link hrefs can no longer inject script.
- The client reconnects automatically with exponential backoff after a dropped connection.
- The document version is now a CRDT counter, so concurrent edits no longer lose bumps.
- The relay drops malformed or oversized document snapshots instead of persisting them,
  and reports users as offline when their connection fails abnormally.
- Split the client into modules (document model, marks, protocol, editor bindings).
- Documented the actual client/relay architecture in README.md and worker/README.md,
  correcting stale docs that described a peer-to-peer WebRTC design no longer used by the
  codebase.
- Cleaned up dead CSS, duplicated field-update logic, and unused dependencies
  (`gloo`, `futures`, `wasm-bindgen-futures`) in the client.
- Fixed a UTF-8 slicing panic risk and a TinyMCE initialization race condition in the client.
