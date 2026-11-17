use super::*;
use crate::content::reason;

#[test]
fn a_renamed_item_is_resolved_by_its_recorded_identity() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let renamed = dir.path().join("renamed");
    fs::write(&original, b"payload").unwrap();
    let location = Location::capture(&original).unwrap();
    fs::rename(&original, &renamed).unwrap();
    let resolved = location.resolve().unwrap();
    assert_eq!(resolved, renamed.canonicalize().unwrap());
}

#[test]
fn a_replacement_at_the_recorded_path_is_never_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let renamed = dir.path().join("renamed");
    fs::write(&original, b"original data").unwrap();
    let location = Location::capture(&original).unwrap();
    fs::rename(&original, &renamed).unwrap();
    fs::write(&original, b"replacement data").unwrap();
    let resolved = location.resolve().unwrap();
    assert_eq!(resolved, renamed.canonicalize().unwrap());
    assert_eq!(fs::read(&resolved).unwrap(), b"original data");
}

#[test]
fn an_item_moved_into_a_subdirectory_is_resolved_by_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(&original, b"payload").unwrap();
    let location = Location::capture(&original).unwrap();
    fs::rename(&original, nested.join("original")).unwrap();
    let resolved = location.resolve().unwrap();
    assert_eq!(resolved, nested.join("original").canonicalize().unwrap());
}

#[test]
fn a_deleted_item_is_reported_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    fs::write(&original, b"payload").unwrap();
    let location = Location::capture(&original).unwrap();
    fs::remove_file(&original).unwrap();
    let error = location.resolve().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(reason(&error), Unavailable::NoSuchItem);
}

#[test]
fn an_item_left_its_directory_is_found_above_it() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let outside = dir.path().join("outside");
    fs::write(&original, b"payload").unwrap();
    let location = Location::capture(&original).unwrap();
    fs::rename(&original, &outside).unwrap();
    let resolved = location.resolve().unwrap();
    assert_eq!(resolved, outside.canonicalize().unwrap());
}
