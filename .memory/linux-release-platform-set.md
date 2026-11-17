---
name: linux-release-platform-set
description: Why the fork's release publishes the Linux packages by default and how the platform set is configured
type: project
---

The fork's release platform set (2026-11-17):

- `build-support/release_platforms.py` is the single source of truth for what a
  release publishes. `parse()` takes the comma-separated `OINK_RELEASE_PLATFORMS`
  repository variable and falls back to `linux-x64,linux-arm64`.
- The guard job in `.github/workflows/release.yml` turns that list into the
  build matrix (`fromJson(needs.guard.outputs.matrix)`), and the draft job
  passes the same list to `public_release.py assemble --platforms`, which
  requires exactly one package record per listed platform.
- macOS is **not** in the default set: `packaging/macos/signing.py` selects the
  certificate for team `893V37X2CW`, which belongs to the upstream author, and
  the notarization step needs that identity's key. A fork cannot produce a
  signed and notarized macOS build, so including `macos` would fail the run.
- Windows is unsigned in the fork unless the SignPath secrets
  (`SIGNPATH_API_TOKEN`, `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`)
  are configured; `public_release.py inputs` requires the DSN
  (`OINK_REPORT_DSN`, EU region) for every release job, and the Windows MSI is
  then published unsigned.

**Why:** the release must publish exactly the platforms it can build, and a
platform that is configured may never be silently missing from a release.
**How to apply:** to publish more platforms, set the `OINK_RELEASE_PLATFORMS`
repository variable to a comma-separated list of `macos`, `macos-x64`,
`windows`, `linux-x64`, `linux-arm64`; never edit the workflow matrix by hand,
because the matrix and the expected records must come from the same list.
See also [[linux-platform-identity]].
