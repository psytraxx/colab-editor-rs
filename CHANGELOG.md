# Changelog

Notable changes to this project, newest first. Entries describe *what* changed, not *how* —
see git history for implementation details. This project doesn't cut versioned releases, so
entries are grouped by date instead.

## 2026-08-18

- Documented the actual client/relay architecture in README.md and worker/README.md,
  correcting stale docs that described a peer-to-peer WebRTC design no longer used by the
  codebase.
- Documented that worker deployment is not currently automated via GitHub Actions and its
  exact mechanism (Cloudflare Git integration vs. manual deploy) is unverified from the repo.
- Cleaned up dead CSS, duplicated field-update logic, and unused dependencies
  (`gloo`, `futures`, `wasm-bindgen-futures`) in the client.
- Fixed a UTF-8 slicing panic risk and a TinyMCE initialization race condition in the client.
