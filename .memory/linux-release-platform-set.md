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
  are configured; the Windows MSI is then published unsigned.
- The reporting DSN (`OINK_REPORT_DSN`, EU region) is required for macOS and
  Windows, and optional for Linux: `public_release.py inputs` takes the
  platform, and `REPORTING_OPTIONAL` names the platforms that may build
  without one. The Linux build then has no reporting configuration
  (`option_env!("OINK_REPORT_DSN")` is empty), the check writes
  `configured: false`, and `stage`/`assemble` accept that proof only for the
  Linux platforms.

**Why:** the release must publish exactly the platforms it can build, and a
platform that is configured may never be silently missing from a release.
**How to apply:** to publish more platforms, set the `OINK_RELEASE_PLATFORMS`
repository variable to a comma-separated list of `macos`, `macos-x64`,
`windows`, `linux-x64`, `linux-arm64`; never edit the workflow matrix by hand,
because the matrix and the expected records must come from the same list.
The release also needs the workflow triggers: a fork that never ran a
workflow does not react to `push` or `pull_request` until a workflow file
changes on the default branch, which re-registers the workflows.

**Republishing a version**: the user keeps the version at `0.3.6` instead of
iterating it, so a fixed build replaces the release of the same version.
`public_release.py publish` refuses to attach the assets to a tag that points
at another commit (`release tag already targets a different commit`) and
`gh release create` refuses an existing release, so the old release and its
tag have to go first:

```
gh release delete v0.3.6 --cleanup-tag --repo kshq1996/oink-linux-support --yes
git push origin main
```

The guard job then finds no release for the version and publishes the new
build under the same tag. The published `release.json` carries the commit the
assets were built from, so the tag of the replaced release moves to the new
commit.

**The republish of v0.3.6 (2026-11-21)**: the release was deleted, the fix was
pushed, and the release was rebuilt with a **higher build sequence** (3): the
workflow was started by hand (`workflow_dispatch` with `sequence: 3`), because
the push trigger always builds sequence 1. The published assets are
`Oink-0.3.6-linux-{x64,arm64}.{rpm,deb}` with the RPM release `3` and the
Debian revision `3` (build `306003`), and the tag points at the commit the
assets were built from. A push after the release exists is harmless: the
guard job sees the release and skips the run.

**The device installs the package with `rpm-ostree`**, not by copying the
binaries: `tools/linux/deploy-device.sh <arch> <host> [--reboot]` downloads the
published RPM, copies it over, runs `sudo rpm-ostree install`, reboots into
the new deployment, refreshes the udev rules
(`udevadm control --reload` + `udevadm trigger --subsystem-match=input
--action=change`), drops the hand-made rule copies in `/etc/udev/rules.d` and
registers the login item through the installed agent. `rpm-ostree` moves the
package's `/opt/oink` to `/usr/lib/opt/oink`, so the installed binaries are
there, not in `/opt/oink`.
See also [[linux-platform-identity]].
