//! Linux identity for a granted item.
//!
//! macOS resolves a granted item with a security-scoped bookmark (and
//! `fsgetpath` for its inode), Windows with the NTFS file ID on its volume.
//! Linux offers no kernel service that maps a recorded inode back to a path
//! (`open_by_handle_at` needs a privilege a desktop session does not have),
//! so the recorded `(st_dev, st_ino)` pair is looked up in the directories
//! that could still hold the item.
//!
//! The lookup compares the recorded identity, never a name: only the exact
//! recorded object matches, so a scan can not be redirected to a different
//! file that took the original's place. Symlinks are never traversed while
//! scanning, so the lookup can not be steered into another tree either. The
//! caller still checks the resolved path against the granted tree, so an item
//! that left the tree is refused instead of read.
//!
//! The lookup costs one directory listing per candidate directory, so it
//! only runs when the recorded path no longer holds the recorded object. It
//! covers the directory that held the item and the directory above it; an
//! item that left both counts as gone, exactly like an item that was deleted.
//! A lookup that did not see every candidate reports a failed lookup instead
//! of a missing item, so an unresolved item never adopts a replacement.
use super::*;
use std::os::unix::fs::MetadataExt;

/// How deep a lookup walks below the directory that held the item.
const MAX_DEPTH: usize = 8;
/// How many directory entries one lookup may stat.
const MAX_ENTRIES: usize = 65_536;

/// The work one lookup may still do. A lookup that runs out of budget did not
/// see every candidate, so its "not found" answer is not conclusive.
struct Budget {
    remaining: usize,
    exhausted: bool,
}

/// Locate the recorded item. The directory that held it is searched first, so
/// a rename inside it still resolves to the original file. The directory above
/// it is searched afterwards, which is where an item that left the granted
/// tree is found; such a path is refused by the caller's tree check.
pub(super) fn resolve(location: &Location) -> io::Result<PathBuf> {
    let identity = location.fingerprint.identity;
    let Some(directory) = location.path.parent() else {
        return Err(unavailable(Unavailable::NoSuchItem));
    };
    // A filesystem root is never descended into: its tree is unbounded.
    let depth = if directory.parent().is_none() { 0 } else { MAX_DEPTH };
    let mut budget = Budget {
        remaining: MAX_ENTRIES,
        exhausted: false,
    };
    if let Some(path) = search(directory, identity, depth, &mut budget) {
        return Ok(path);
    }
    if let Some(parent) = directory.parent() {
        let mut above = Budget {
            remaining: MAX_ENTRIES,
            exhausted: false,
        };
        if let Some(path) = search(parent, identity, 0, &mut above) {
            return Ok(path);
        }
        budget.exhausted |= above.exhausted;
    }
    if budget.exhausted {
        // The item may still exist in a directory the lookup did not reach.
        // Report a failed lookup, never a missing item: a missing item would
        // let the grant adopt whatever now occupies the recorded path.
        return Err(unavailable(Unavailable::ReadFailed));
    }
    Err(unavailable(Unavailable::NoSuchItem))
}

fn search(directory: &Path, identity: [u64; 2], depth: usize, budget: &mut Budget) -> Option<PathBuf> {
    let mut nested = Vec::new();
    for entry in fs::read_dir(directory).ok()?.flatten() {
        if budget.remaining == 0 {
            budget.exhausted = true;
            return None;
        }
        budget.remaining -= 1;
        let path = entry.path();
        // `DirEntry::metadata` does not follow symlinks on Unix, so a link is
        // compared by its own identity and never traversed.
        let Ok(metadata) = entry.metadata() else { continue };
        if metadata.dev() == identity[0] && metadata.ino() == identity[1] {
            return Some(path);
        }
        if depth > 0 && metadata.is_dir() {
            nested.push(path);
        }
    }
    // The directory itself is searched before its subdirectories, so a rename
    // inside the directory that held the item is found first.
    for path in nested {
        if let Some(found) = search(&path, identity, depth - 1, budget) {
            return Some(found);
        }
    }
    None
}

#[cfg(all(test, debug_assertions))]
mod tests;
