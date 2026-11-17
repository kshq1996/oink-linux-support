---
name: linux-test-suite
description: How the Linux suite runs in this fork, and why an emulated amd64 container reports failures that native runners do not
type: project
---

The Linux test suite of this fork (2026-11-17):

- `tools/linux/run-docker-tests.sh --arch <amd64|arm64> [--deny]` runs the whole
  suite in the project image: the frontend (typecheck, vitest), `cargo fmt`,
  clippy, the workspace tests, the release-profile parser tests, the packaging
  tests and `cargo deny check`. `tools/linux/check.sh <cargo args>` runs a
  single cargo command, and `tools/linux/clean.sh --all` removes the images
  and the cached cargo/target volumes.
- The workspace image is `oink-linux-build:0.3.6-<arch>`; the amd64 image has
  to be built with `docker buildx build --platform linux/amd64` because the
  workstation is arm64.
- **A full suite under an emulated amd64 container produces false failures.**
  On this workstation the amd64 run is Rosetta-emulated and 13 timing-sensitive
  agent tests (pairing, clipboard file transfer, projection, audio, IPC) time
  out; the same tests pass when they run alone and the whole suite passes on
  the native arm64 runner. Trust the native runners and
  `OINK_LINUX_ARCH=arm64 tools/linux/run-docker-tests.sh --arch arm64`.
- The CI runs the same script on `ubuntu-24.04` (amd64) and
  `ubuntu-24.04-arm` (arm64); both are native.

**Why:** the emulated failures look like port defects and cost a full debug
cycle before the emulation was identified as the cause.
**How to apply:** reproduce a Linux test failure on the native arm64 runner
first; only treat it as a port defect when it fails there too.
See also [[linux-x11-input-backend]] and [[linux-release-platform-set]].
