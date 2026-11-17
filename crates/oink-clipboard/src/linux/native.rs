//! The X11 selection as a native clipboard inventory.
//!
//! arboard owns the selection and the MIME targets it publishes. Every target
//! the selection offers becomes one representation: the target name is the
//! native format identity, and the portable contract a foreign peer renders is
//! the standard format. The Linux clipboard has no private format registry, so
//! the marker of a write this device published stays in process memory and is
//! announced next to the content it belongs to.
use std::io::{self, Read};
use std::path::PathBuf;

use oink_foundation::clipboard::ClipPlatform;
use oink_foundation::content::OfferId;

use crate::content::{self, Translator};
use crate::model::{
    ContentItem, ContentSize, Inventory, ItemId, Medium, NativeFormat, NodeId, Representation, RepresentationData,
    RepresentationId, StandardFormat, Unavailable,
};
use crate::{ClipData, ClipboardError, Sensitivity};

use super::Publication;

/// The MIME target of plain text.
pub(super) const TEXT_TARGET: &str = "text/plain;charset=utf-8";
/// The MIME target of an HTML fragment.
pub(super) const HTML_TARGET: &str = "text/html;charset=utf-8";
/// The MIME target of a PNG image.
pub(super) const PNG_TARGET: &str = "image/png";
/// The MIME target of a file list.
pub(super) const FILE_TARGET: &str = "text/uri-list";

/// The current selection: one native item, its representations and the roots
/// of a file list.
pub(super) struct Selection {
    pub inventory: Inventory,
    /// The bytes of every byte representation, by representation id.
    pub bytes: Vec<(RepresentationId, Vec<u8>)>,
    /// The local paths of a file list.
    pub roots: Vec<(NodeId, PathBuf)>,
}

/// Build the inventory of the current selection.
///
/// A file list is a file item; text, HTML and images are byte representations
/// of the same item. The Oink clip marker of a write this device published is
/// announced next to the content it belongs to, and never leaves this device.
/// A concealed selection carries the privacy hint, so a peer keeps it out of
/// its history as well.
pub(super) fn selection(
    items: &[ClipData],
    id: OfferId,
    marker: Option<&[u8]>,
    sensitivity: Sensitivity,
) -> Result<Selection, ClipboardError> {
    let mut next = 0_u32;
    let mut representations: Vec<Representation> = Vec::new();
    let mut bytes: Vec<(RepresentationId, Vec<u8>)> = Vec::new();
    let mut roots: Vec<(NodeId, PathBuf)> = Vec::new();
    let mut file = None;

    for item in items {
        let (target, standard, payload) = match item {
            ClipData::Text(text) => (TEXT_TARGET, StandardFormat::Text, text.as_bytes().to_vec()),
            ClipData::Html(html) => (HTML_TARGET, StandardFormat::Html, html.as_bytes().to_vec()),
            ClipData::Png(png) => (PNG_TARGET, StandardFormat::Png, png.clone()),
            ClipData::Files(entries) => {
                for entry in entries {
                    let Some(path) = entry.local_path.clone() else {
                        continue;
                    };
                    roots.push((NodeId(roots.len() as u64 + 1), path));
                }
                file = Some(NodeId(1));
                next += 1;
                representations.push(Representation {
                    id: RepresentationId(next),
                    standard: None,
                    format: NativeFormat::Linux(FILE_TARGET.into()),
                    medium: Medium::Bytes,
                    aspect: 1,
                    index: -1,
                    data: RepresentationData::File(NodeId(1)),
                });
                continue;
            }
            // A custom format has no Linux target, and a native item of
            // another platform never reaches this selection.
            _ => continue,
        };
        next += 1;
        let id = RepresentationId(next);
        let size = payload.len() as u64;
        bytes.push((id, payload));
        representations.push(Representation {
            id,
            standard: Some(standard),
            format: NativeFormat::Linux(target.into()),
            medium: Medium::Bytes,
            aspect: 1,
            index: -1,
            data: RepresentationData::Reader {
                size: ContentSize::Known(size),
            },
        });
    }

    let mut inventory = Inventory {
        id,
        platform: ClipPlatform::Linux,
        items: vec![ContentItem {
            virtual_files: false,
            id: ItemId(0),
            file,
            representations,
        }],
        roots: Vec::new(),
    };
    if let Some(marker) = marker {
        // One private format next to the content, as the native writers do.
        next += 1;
        let id = RepresentationId(next);
        bytes.push((id, marker.to_vec()));
        inventory.items.push(ContentItem {
            virtual_files: false,
            id: ItemId(next),
            file: None,
            representations: vec![Representation {
                id,
                standard: None,
                format: NativeFormat::Linux(crate::dedupe::MARKER_LINUX.into()),
                medium: Medium::Bytes,
                aspect: 1,
                index: -1,
                data: RepresentationData::Reader {
                    size: ContentSize::Known(marker.len() as u64),
                },
            }],
        });
    }
    if sensitivity == Sensitivity::Concealed {
        // The privacy hint travels with the content, so a peer keeps it out of
        // its history as well.
        next += 1;
        let id = RepresentationId(next);
        bytes.push((id, b"secret".to_vec()));
        inventory.items.push(ContentItem {
            virtual_files: false,
            id: ItemId(next),
            file: None,
            representations: vec![Representation {
                id,
                standard: Some(StandardFormat::Privacy),
                format: NativeFormat::Linux(crate::formats::LINUX_MARKER.into()),
                medium: Medium::Bytes,
                aspect: 1,
                index: -1,
                data: RepresentationData::Reader {
                    size: ContentSize::Known(6),
                },
            }],
        });
    }
    inventory.validate().map_err(ClipboardError::ContentUnavailable)?;
    Ok(Selection {
        inventory,
        bytes,
        roots,
    })
}

/// The bytes of one representation, decoded into the shared content model.
///
/// A same-platform offer carries the native target bytes; a foreign offer
/// carries exactly the declared portable bytes.
pub(super) fn decode(representation: &Representation, bytes: &[u8], foreign: bool) -> Result<ClipData, ClipboardError> {
    if representation.standard == Some(StandardFormat::Url) {
        // A link travels as its own portable record; Linux publishes it as
        // text, the only target its clipboard can offer.
        let link = content::url::Link::decode(bytes)
            .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?;
        return Ok(ClipData::Text(link.url));
    }
    let decoded = if foreign {
        content::decode_standard(representation, bytes)
    } else {
        LinuxTranslator.standard(representation, bytes)
    }
    .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?;
    decoded
        .into_iter()
        .next()
        .ok_or(ClipboardError::ContentUnavailable(Unavailable::Unsupported))
}

/// The native bytes of a Linux target are the portable bytes of its standard
/// format, so the same decoder serves both directions.
pub(super) struct LinuxTranslator;

impl Translator for LinuxTranslator {
    fn standard(&self, representation: &Representation, bytes: &[u8]) -> io::Result<Vec<ClipData>> {
        let text = || String::from_utf8(bytes.to_vec()).map_err(|_| content::unavailable(Unavailable::InvalidData));
        Ok(vec![match representation.standard {
            Some(StandardFormat::Text) => ClipData::Text(text()?),
            Some(StandardFormat::Html) => ClipData::Html(text()?),
            Some(StandardFormat::Rtf) => ClipData::Rtf(bytes.to_vec()),
            Some(StandardFormat::Png) => ClipData::Png(bytes.to_vec()),
            // The privacy hint is metadata: it names the target, not the content.
            Some(StandardFormat::Privacy) => {
                let NativeFormat::Linux(name) = &representation.format else {
                    return Err(content::unavailable(Unavailable::Unsupported));
                };
                ClipData::Custom {
                    platform: ClipPlatform::Linux,
                    name: name.clone(),
                    bytes: bytes.to_vec(),
                }
            }
            _ => return Err(content::unavailable(Unavailable::Unsupported)),
        }])
    }
}

/// The publication a received offer asks for: the platform expresses one
/// target at a time, so the richest usable representation wins.
pub(super) fn publication(specification: &crate::NativeSpec) -> Result<Option<Publication>, ClipboardError> {
    let foreign = specification.inventory.platform != ClipPlatform::Linux;
    let mut items = Vec::new();
    for representation in specification.inventory.wanted(ClipPlatform::Linux) {
        // The Oink marker is metadata, not content: it never becomes a target.
        if crate::dedupe::is_marker(&representation.format) {
            continue;
        }
        if matches!(
            representation.data,
            RepresentationData::File(_) | RepresentationData::Promise(..)
        ) {
            continue;
        }
        let mut bytes = Vec::new();
        specification
            .contents
            .open(representation.id)
            .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?
            .read_to_end(&mut bytes)
            .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?;
        items.push(decode(representation, &bytes, foreign)?);
    }
    if !specification.files.is_empty() {
        items.push(ClipData::Files(specification.files.clone()));
    }
    Ok(super::plan_publication(&items))
}

// 无 libtest 的构建会去掉 `#[test]` 的函数体，这些辅助函数因此没人调用。
#[cfg(all(test, debug_assertions))]
#[allow(dead_code)]
mod tests {

    use std::collections::HashMap;
    use std::sync::Arc;

    use oink_foundation::DeviceId;
    use oink_foundation::content::{ClipId, OfferId};

    use super::*;

    fn offer() -> OfferId {
        OfferId {
            clip: ClipId {
                origin: DeviceId([0; 32]),
                seq: 0,
            },
            generation: 1,
        }
    }

    /// A provider that serves the bytes of a built inventory.
    struct Bytes {
        inventory: Inventory,
        bytes: HashMap<RepresentationId, Vec<u8>>,
    }

    impl content::Provider for Bytes {
        fn inventory(&self) -> Inventory {
            self.inventory.clone()
        }

        fn open(&self, id: RepresentationId) -> io::Result<crate::FileReader> {
            let bytes = self
                .bytes
                .get(&id)
                .cloned()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            Ok(Box::new(io::Cursor::new(bytes)))
        }

        fn translator(&self) -> Arc<dyn Translator> {
            Arc::new(LinuxTranslator)
        }
    }

    fn specification(selection: &Selection) -> crate::NativeSpec {
        crate::NativeSpec {
            inventory: selection.inventory.clone(),
            contents: Arc::new(Bytes {
                inventory: selection.inventory.clone(),
                bytes: selection.bytes.iter().cloned().collect(),
            }),
            files: Vec::new(),
            file_contents: None,
            sensitivity: crate::Sensitivity::Normal,
        }
    }

    #[test]
    fn selection_describes_the_x11_targets() {
        let items = vec![
            ClipData::Text("文本".into()),
            ClipData::Html("<p>文本</p>".into()),
            ClipData::Png(vec![1, 2, 3]),
        ];
        let selection = selection(&items, offer(), None, Sensitivity::Normal).unwrap();
        assert_eq!(selection.inventory.platform, ClipPlatform::Linux);
        assert_eq!(selection.inventory.items.len(), 1);
        let representations = &selection.inventory.items[0].representations;
        assert_eq!(representations.len(), 3);
        assert_eq!(representations[0].format, NativeFormat::Linux(TEXT_TARGET.into()));
        assert_eq!(representations[0].standard, Some(StandardFormat::Text));
        assert_eq!(representations[1].format, NativeFormat::Linux(HTML_TARGET.into()));
        assert_eq!(representations[1].standard, Some(StandardFormat::Html));
        assert_eq!(representations[2].format, NativeFormat::Linux(PNG_TARGET.into()));
        assert_eq!(representations[2].standard, Some(StandardFormat::Png));
        assert_eq!(selection.bytes.len(), 3);
    }

    #[test]
    fn selection_announces_the_marker_next_to_the_content() {
        let items = vec![ClipData::Text("文本".into())];
        let selection = selection(&items, offer(), Some(b"marker"), Sensitivity::Normal).unwrap();
        assert_eq!(selection.inventory.items.len(), 2);
        let marker = &selection.inventory.items[1];
        assert_eq!(marker.representations.len(), 1);
        assert_eq!(
            marker.representations[0].format,
            NativeFormat::Linux(crate::dedupe::MARKER_LINUX.into())
        );
        assert_eq!(marker.representations[0].standard, None);
    }

    #[test]
    fn selection_describes_a_file_list() {
        let items = vec![ClipData::Files(vec![crate::FileEntry {
            path: "a.txt".into(),
            is_dir: false,
            size: 1,
            modified_ms: None,
            local_path: Some(PathBuf::from("/tmp/a.txt")),
        }])];
        let selection = selection(&items, offer(), None, Sensitivity::Normal).unwrap();
        assert_eq!(selection.roots, vec![(NodeId(1), PathBuf::from("/tmp/a.txt"))]);
        assert_eq!(selection.inventory.items[0].file, Some(NodeId(1)));
        let representations = &selection.inventory.items[0].representations;
        assert_eq!(representations.len(), 1);
        assert_eq!(representations[0].data, RepresentationData::File(NodeId(1)));
        assert_eq!(representations[0].format, NativeFormat::Linux(FILE_TARGET.into()));
    }

    #[test]
    fn publication_publishes_the_richest_representation() {
        let items = vec![ClipData::Text("文本".into())];
        let selection = selection(&items, offer(), None, Sensitivity::Normal).unwrap();
        let specification = specification(&selection);
        assert_eq!(
            publication(&specification).unwrap(),
            Some(Publication::Text("文本".into()))
        );
    }

    #[test]
    fn publication_skips_the_marker() {
        let items = vec![ClipData::Text("文本".into())];
        let selection = selection(&items, offer(), Some(b"marker"), Sensitivity::Normal).unwrap();
        let specification = specification(&selection);
        assert_eq!(
            publication(&specification).unwrap(),
            Some(Publication::Text("文本".into()))
        );
    }

    #[test]
    fn publication_answers_a_foreign_peer_with_the_portable_contract() {
        let inventory = Inventory {
            id: offer(),
            platform: ClipPlatform::Mac,
            items: vec![ContentItem {
                virtual_files: false,
                id: ItemId(0),
                file: None,
                representations: vec![Representation {
                    id: RepresentationId(1),
                    standard: Some(StandardFormat::Text),
                    format: NativeFormat::Mac("public.utf8-plain-text".into()),
                    medium: Medium::Bytes,
                    aspect: 1,
                    index: -1,
                    data: RepresentationData::Reader {
                        size: ContentSize::Known(6),
                    },
                }],
            }],
            roots: Vec::new(),
        };
        let specification = crate::NativeSpec {
            inventory: inventory.clone(),
            contents: Arc::new(Bytes {
                inventory,
                bytes: HashMap::from([(RepresentationId(1), "文本".as_bytes().to_vec())]),
            }),
            files: Vec::new(),
            file_contents: None,
            sensitivity: crate::Sensitivity::Normal,
        };
        assert_eq!(
            publication(&specification).unwrap(),
            Some(Publication::Text("文本".into()))
        );
    }

    #[test]
    fn selection_announces_the_privacy_hint_of_a_concealed_selection() {
        let items = vec![ClipData::Text("秘密".into())];
        let selection = selection(&items, offer(), None, Sensitivity::Concealed).unwrap();
        assert_eq!(selection.inventory.items.len(), 2);
        let privacy = &selection.inventory.items[1];
        assert_eq!(privacy.representations[0].standard, Some(StandardFormat::Privacy));
        assert_eq!(
            privacy.representations[0].format,
            NativeFormat::Linux(crate::formats::LINUX_MARKER.into())
        );
        assert!(crate::formats::is_marker(
            ClipPlatform::Linux,
            crate::formats::LINUX_MARKER
        ));
    }

    #[test]
    fn the_privacy_hint_of_the_selection_is_read_back() {
        if std::env::var_os("DISPLAY").is_none() {
            // 没有 X 会话就没有选择可读。
            return;
        }
        use arboard::SetExtLinux;
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        clipboard
            .set()
            .exclude_from_history()
            .text("秘密")
            .expect("the selection takes the text");
        assert!(crate::linux::privacy::concealed());
        clipboard.set().text("普通").expect("the selection takes the text");
        assert!(!crate::linux::privacy::concealed());
    }
}
