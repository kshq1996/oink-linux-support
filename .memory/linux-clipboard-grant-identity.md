---
name: linux-clipboard-grant-identity
description: How the oink-clipboard Linux port resolves granted file identity, its symlink rule, and the NFC dependency
type: project
---

Facts learned while fixing the Linux grant identity in `crates/oink-clipboard` (2026-11-17):

- macOS resolves a granted item with a security-scoped bookmark (`location.rs` `mac::Identity`) plus
  `fsgetpath` on the volume's fsid; Windows uses the NTFS file ID via `OpenFileById`. Linux has no
  kernel service for inode → path (`open_by_handle_at` needs CAP_DAC_READ_SEARCH), so the port looks
  the recorded `(st_dev, st_ino)` pair up by scanning the directory that held the item and the
  directory above it (`crates/oink-clipboard/src/content/grants/location/linux.rs`).
- The scan matches the **identity, never the name**, and never follows symlinks (`DirEntry::metadata`
  is lstat-based on Unix). A path the scan returns is still checked by the shared grant tree check in
  `grants.rs::resolve`, so an item that left the granted tree is refused with `Permission`, never read.
- `Location::resolve` keeps a shared "atomic save" fallback: when the identity lookup reports *gone*,
  the path that used to hold the item may be adopted. That is why an inconclusive lookup must report
  `ReadFailed` (not `NoSuchItem`): `NoSuchItem` would let a replacement be adopted while the original
  still exists.
- Linux does not traverse a symlink to a **directory** (`Batch::capture` in
  `content/namespace/local/batch.rs`); a symlink to a regular file is still followed. This is what the
  shared test `external_links_follow_but_replacing_an_ancestor_cannot_redirect_a_reader` expects
  (`macos` = 2 entries, other platforms = 1).
- `received::normalized_name` needs NFC on Linux; the smallest dependency is `unicode-normalization`
  (MIT OR Apache-2.0, no transitive deps), added as a `cfg(target_os = "linux")` dependency so the
  macOS/Windows graphs stay untouched. `Cargo.lock`, `third-party-licenses.json` and
  `THIRD_PARTY_LICENSES.md` were regenerated afterwards, so the Linux-only crates (`unicode-normalization`,
  `arboard`, `x11rb`) are inventoried; `about.toml` lists the Linux targets so `cargo about` keeps them.

**Why:** the shared grant tests encode the product's identity contract, and the Linux port has no
platform primitive for it; the scan is the only affordable equivalent.
**How to apply:** keep Linux-only behaviour inside `#[cfg(target_os = "linux")]` branches or new
Linux-only modules; verify with `OINK_LINUX_ARCH=arm64 tools/linux/check.sh {test,check,clippy} -p oink-clipboard`.
See also [[linux-x11-input-backend]].
