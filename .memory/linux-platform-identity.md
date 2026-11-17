---
name: linux-platform-identity
description: Which platform identity each Linux-enabled subsystem reports, and why the protocol schema stays frozen
type: project
---

Facts about the Linux port's platform identity (2026-11-17):

- `oink_routing::Os` gained a `Linux` arm (`crates/oink-routing/src/config.rs`).
  `as_str()` returns `"linux"`, which is what `DeviceFacts.os` carries (a free
  `Text<256>` field, so no schema change). `parse("linux")` maps a peer's
  advertisement back to the Linux arm.
- The Linux build routes like the generic class: every `match` on `Os` in the
  router keeps its catch-all branch, so Linux takes the same path `Other`
  took. The arm exists for *identity*, not for new routing behaviour.
- The v1 protocol schema is **frozen**: `crates/oink-protocol/tests/fixtures/v1/manifest.json`
  is marked frozen and the golden test asserts the schema hash, so the
  diagnostics enum `DiagOs` (MacOS/Windows/Other) cannot gain a Linux value.
  `crates/oink-agent/src/diagnostics/wire_values.rs` maps `Os::Linux` to
  `DiagOs::Other` for that reason.
- The signed update catalog is the macOS release feed. `crates/oink-agent/src/update/online.rs`
  only attaches its timer on macOS (`cfg!(target_os = "macos")`), and the
  manual check now answers `Error::OsUnsupported` on Linux; Linux packages are
  updated by the distribution package manager.

**Why:** a Linux build must not advertise itself as "other" to its peers, but
the frozen wire schema and the macOS-only update feed must not be changed by a
platform port.
**How to apply:** keep new platform identity in domain enums and free-form
strings; extend a frozen wire enum only together with a schema unfreeze.
See also [[linux-x11-selection-privacy]].
