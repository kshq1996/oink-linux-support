---
name: linux-dependency-policy
description: How the Linux port affects cargo-deny policy, and the two advisories that block it
type: project
---

Dependency-policy facts for the Linux port (2026-11-17):

- `deny.toml` audits four targets: `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu`. The Linux pair makes the graph include GTK,
  WebKitGTK, ALSA, `arboard` and `x11rb`, so the policy check has to resolve
  those crates on the macOS and Windows runners too.
- `RUSTSEC-2024-0370` (`proc-macro-error` unmaintained) is ignored in
  `deny.toml`. It is not a vulnerability and has no fix: it only enters the
  graph through `glib-macros`, which the Linux system tray pulls in via GTK.
  It is a build-time dependency and never ships in a binary.
- `quinn-proto` was bumped to 0.11.19 because 0.11.18 is covered by a
  security advisory (an Initial token that leaves too little room for frames).
  The bump is a patch release inside the same `0.11.x` range, and it is the
  reason `quinn-udp` moved off 0.5.15 in the license inventory.
- `cargo about` needs the license texts of crates that are only published for
  other platforms (the `rolldown`/`tauri-apps`/`lightningcss` native
  bindings). `build-support/generate-licenses.mjs` caches them under
  `$TMPDIR/oink-license-npm`, named by the sha256 of the lockfile integrity
  string; a slow registry makes its 60 s fetch timeout fail the run.

**Why:** the Linux port changes the audited dependency graph, so the policy
gate and the attribution inventory both had to move with it; the two
advisories are the only blockers and both need a deliberate decision.
**How to apply:** re-run `cargo deny check` and
`node build-support/generate-licenses.mjs --check --release-sources` after
changing any Linux dependency or the lockfile. See [[linux-udp-probe-icmp-refusal]].
