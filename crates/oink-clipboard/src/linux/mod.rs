//! Linux（X11）剪贴板实现：通过 [`arboard`] 的 X11 selection 协议读写 `CLIPBOARD`。
//!
//! - 读：依次探测 `image/png`、`text/uri-list`、`text/html`、`UTF8_STRING`，取到哪种就返回哪种；
//!   剪贴板为空时返回空快照，而不是错误。文件清单排在纯文本前面，因为很多工具同时发布
//!   `text/uri-list` 与纯文本，文件清单才是更精确的那一份。
//! - 写：arboard 的一次 `Set` 只能发布一组目标，因此 Linux 上一次只发布一种表示，
//!   优先顺序是图片 → 文本（HTML 连同纯文本备选）→ 文件清单。
//! - 变化：X11 没有 selection 变化事件，只能轮询内容身份（blake3），间隔不超过 250 ms。
//!   身份相同就不算变化；内容变了计数加一，并通知所有已注册的回调。
//! - 隐私：写入 `Concealed` 时附带 `x-kde-passwordManagerHint`，让桌面剪贴板管理器不把它写进历史；
//!   读取时 X11 没有统一的隐私标记，一律按 `Normal` 返回。
//!
//! 不在本模块范围内：虚拟文件发布（[`ClipboardBackend::write_virtual_files`] 保持 trait 的默认实现，
//! 返回 `ClipboardError::Unsupported`）、文件传输 promise，以及拖放后端
//! （[`crate::dnd`] 的 `DndBackend` 是另一个 trait，与本模块无关）。
// 本模块在 Linux 上全部使用，不允许被 crate 级的平台 allow 掩盖。
#![deny(dead_code, unused_imports)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arboard::{Clipboard as Arboard, Error as ArboardError, ImageData, SetExtLinux};
use blake3::Hasher;
use oink_foundation::DeviceId;
use oink_foundation::clipboard::ClipPlatform;
use oink_foundation::content::{ClipId, OfferId};

use crate::content::{self, Provider, ReaderGuard};
use crate::model::{ContentSize, NodeKind, RepresentationData, Unavailable};
use crate::{
    ChangeHandler, ClipData, ClipSnapshot, ClipboardBackend, ClipboardError, FileEntry, NativeSpec, Sensitivity,
};

mod native;
mod privacy;

/// 轮询间隔。X11 没有 selection 变化通知，只能轮询；上限与 macOS 后端一致（≤ 250 ms）。
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 创建本平台的剪贴板实现。
pub(crate) fn backend() -> Result<Box<dyn ClipboardBackend>, ClipboardError> {
    let clipboard =
        Arboard::new().map_err(|error| ClipboardError::Platform(format!("打开 X11 剪贴板失败：{error}")))?;
    Ok(Box::new(LinuxClipboard {
        inner: Arc::new(Inner {
            clipboard: Mutex::new(clipboard),
            state: Mutex::new(ChangeState::default()),
            handlers: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            watcher: Mutex::new(None),
        }),
    }))
}

/// 一次发布的内容。arboard 的 `Set` 一次只能写一组目标，所以只发布一种表示。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Publication {
    /// 图片：项目模型里的 PNG 字节。
    Image(Vec<u8>),
    /// HTML 片段，以及可选的纯文本备选。
    Html { html: String, alt: Option<String> },
    /// 纯文本。
    Text(String),
    /// 文件清单：本机绝对路径。
    Files(Vec<PathBuf>),
}

/// 变化计数、最近一次内容身份与暂停标志。纯状态，不接触 X11，便于单测。
#[derive(Debug)]
struct ChangeState {
    /// 最近一次观察到的内容身份；`None` 表示还没有基线。
    last: Option<[u8; 32]>,
    /// 变化计数，单调递增。
    count: u64,
    /// 暂停标志：暂停期间不轮询，也不产生新的计数。
    paused: bool,
    /// 本进程最后一次发布的内容：它的身份、Oink 标记与隐私级别。
    written: Option<Written>,
}

/// 本进程发布的内容。X11 没有私有格式注册表，Oink 标记只存在于进程内存里，
/// 下一次捕获时跟着同一份内容一起公布。
#[derive(Clone, Debug)]
struct Written {
    key: [u8; 32],
    marker: Option<Vec<u8>>,
    sensitivity: Sensitivity,
}

impl Default for ChangeState {
    fn default() -> Self {
        Self {
            last: None,
            count: 1,
            paused: false,
            written: None,
        }
    }
}

impl ChangeState {
    /// 记录基线身份，不产生计数（第一次注册监听时用，避免把初始内容当成一次变化）。
    fn baseline(&mut self, key: [u8; 32]) {
        self.last = Some(key);
    }

    /// 本进程发布内容：计数加一，并把内容身份记为已观察。
    fn published(&mut self, key: [u8; 32]) -> u64 {
        self.last = Some(key);
        self.count += 1;
        self.forget(key);
        self.count
    }

    /// 观察剪贴板当前身份；内容变了才返回新的计数。
    fn observed(&mut self, key: [u8; 32]) -> Option<u64> {
        if self.last == Some(key) {
            return None;
        }
        self.last = Some(key);
        self.count += 1;
        self.forget(key);
        Some(self.count)
    }

    /// 本进程发布的内容：身份、Oink 标记与隐私级别。
    fn written(&self) -> Option<&Written> {
        self.written.as_ref()
    }

    /// 记住本进程发布的内容。
    fn wrote(&mut self, written: Written) {
        self.written = Some(written);
    }

    /// 内容换成了别的东西：之前的标记不再属于它。
    fn forget(&mut self, key: [u8; 32]) {
        if self.written.as_ref().is_some_and(|written| written.key != key) {
            self.written = None;
        }
    }

    fn count(&self) -> u64 {
        self.count
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    fn paused(&self) -> bool {
        self.paused
    }
}

struct Inner {
    /// arboard 句柄。X11 上剪贴板内容由持有 selection 的进程提供，因此这个句柄必须一直活着。
    clipboard: Mutex<Arboard>,
    /// 变化计数、内容身份与暂停标志。
    state: Mutex<ChangeState>,
    /// 已注册的变化回调。
    handlers: Mutex<Vec<ChangeHandler>>,
    /// 暂停/恢复的唤醒信号。
    wake: Condvar,
    /// 监听线程的停止标志。
    stop: AtomicBool,
    /// 监听线程。
    watcher: Mutex<Option<JoinHandle<()>>>,
}

struct LinuxClipboard {
    inner: Arc<Inner>,
}

impl ClipboardBackend for LinuxClipboard {
    fn change_count(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .count()
    }

    fn clipboard_key(&self) -> std::io::Result<[u8; 32]> {
        let items = self
            .read_items()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(content_key(&items))
    }

    fn read(&self) -> Result<ClipSnapshot, ClipboardError> {
        let items = self.read_items()?;
        let count = self.change_count();
        Ok(ClipSnapshot {
            items,
            sensitivity: Sensitivity::Normal,
            change_count: count,
        })
    }

    fn write(&self, items: &[ClipData], sensitivity: Sensitivity) -> Result<u64, ClipboardError> {
        let publication = plan_publication(items).ok_or(ClipboardError::Unsupported)?;
        let mut clipboard = self.inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
        publish(&mut clipboard, &publication, sensitivity)?;
        // 读回真实内容作为身份基线，避免把自己的写入当成一次外部变化。
        let key = match read_items(&mut clipboard) {
            Ok(read) => content_key(&read),
            Err(_) => content_key(&published_items(&publication)),
        };
        let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
        Ok(state.published(key))
    }

    /// 捕获当前选择：一份清单，公开的表示形式带可移植契约，文件清单交给命名空间。
    fn capture(&self, _data: &std::path::Path) -> Result<crate::CapturedClipboard, ClipboardError> {
        let mut clipboard = self.inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
        let items = read_items(&mut clipboard)?;
        let count = self.change_count();
        let id = OfferId {
            clip: ClipId {
                origin: DeviceId([0; 32]),
                seq: 0,
            },
            generation: count,
        };
        let key = content_key(&items);
        let (marker, sensitivity) = {
            let state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
            match state.written() {
                // 自己发布的内容：隐私级别就在发布的那一刻定下来了。
                Some(written) if written.key == key => (written.marker.clone(), written.sensitivity),
                // 别人复制的内容：所有者用 KDE 的提示目标说明它是不是秘密。
                _ => (
                    None,
                    if privacy::concealed() {
                        Sensitivity::Concealed
                    } else {
                        Sensitivity::Normal
                    },
                ),
            }
        };
        let selection = native::selection(&items, id, marker.as_deref(), sensitivity)?;
        let files = if selection.roots.is_empty() {
            None
        } else {
            Some(
                content::namespace::Local::clipboard(selection.roots.clone())
                    .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?
                    as Arc<dyn content::namespace::Namespace>,
            )
        };
        let bytes: HashMap<_, _> = selection.bytes.into_iter().collect();
        // 读取前重新确认选择没有变：变了就说明捕获到的字节已经过期。
        let check: content::clipboard::Check = {
            let inner = self.inner.clone();
            Arc::new(move || {
                let mut clipboard = inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
                let current = read_items(&mut clipboard).map_err(|error| io::Error::other(error.to_string()))?;
                if content_key(&current) == key {
                    Ok(())
                } else {
                    Err(content::clipboard::changed())
                }
            })
        };
        let provider = content::clipboard::Clipboard::new(
            selection.inventory,
            Box::new(move |id| {
                bytes
                    .get(&id)
                    .map(|bytes| Box::new(io::Cursor::new(bytes.clone())) as crate::FileReader)
                    .ok_or_else(|| content::unavailable(Unavailable::NoSuchItem))
            }),
            check,
            Arc::new(native::LinuxTranslator),
            files,
        )
        .map_err(|error| ClipboardError::ContentUnavailable(content::reason(&error)))?;
        Ok(crate::CapturedClipboard {
            snapshot: ClipSnapshot {
                items: content::items(&provider.inventory()),
                sensitivity,
                change_count: count,
            },
            content: Some(provider),
        })
    }

    /// 把收到的报价写进选择：一次只发布一种表示，图片优先，其次 HTML、文本、文件。
    fn write_native_guarded(&self, spec: NativeSpec, guard: ReaderGuard) -> Result<u64, ClipboardError> {
        let publication = native::publication(&spec)?.ok_or(ClipboardError::Unsupported)?;
        let mut clipboard = self.inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
        guard().map_err(|_| ClipboardError::Busy)?;
        publish(&mut clipboard, &publication, spec.sensitivity)?;
        // 读回真实内容作为身份基线，避免把自己的写入当成一次外部变化。
        let key = match read_items(&mut clipboard) {
            Ok(read) => content_key(&read),
            Err(_) => content_key(&published_items(&publication)),
        };
        let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
        let count = state.published(key);
        state.wrote(Written {
            key,
            marker: crate::dedupe::marker(spec.inventory.id),
            sensitivity: spec.sensitivity,
        });
        Ok(count)
    }

    fn watch(&self, on_change: ChangeHandler) -> Result<(), ClipboardError> {
        let mut watcher = self.inner.watcher.lock().unwrap_or_else(|error| error.into_inner());
        self.inner
            .handlers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(on_change);
        if watcher.is_none() {
            // 以当前内容作为基线，避免第一次轮询就把初始内容当成一次变化。
            if let Some(key) = current_key(&self.inner) {
                self.inner
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .baseline(key);
            }
            *watcher = Some(spawn_watcher(self.inner.clone())?);
        }
        Ok(())
    }

    fn set_watch_paused(&self, paused: bool) -> Result<(), ClipboardError> {
        let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
        state.set_paused(paused);
        // 恢复时立刻唤醒监听线程，重新检查一次内容。
        self.inner.wake.notify_all();
        Ok(())
    }
}

impl LinuxClipboard {
    /// 读取剪贴板当前持有的表示形式。
    fn read_items(&self) -> Result<Vec<ClipData>, ClipboardError> {
        let mut clipboard = self.inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
        read_items(&mut clipboard)
    }
}

impl Drop for LinuxClipboard {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        self.inner.wake.notify_all();
        if let Some(handle) = self
            .inner
            .watcher
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}

/// 从待发布的内容里选出平台能表达的一种表示：图片 → 文本 → 文件清单。
fn plan_publication(items: &[ClipData]) -> Option<Publication> {
    if let Some(png) = items.iter().find_map(|item| match item {
        ClipData::Png(bytes) => Some(bytes.clone()),
        _ => None,
    }) {
        return Some(Publication::Image(png));
    }
    let html = items.iter().find_map(|item| match item {
        ClipData::Html(html) => Some(html.clone()),
        _ => None,
    });
    let text = items.iter().find_map(|item| match item {
        ClipData::Text(text) => Some(text.clone()),
        _ => None,
    });
    if let Some(html) = html {
        return Some(Publication::Html { html, alt: text });
    }
    if let Some(text) = text {
        return Some(Publication::Text(text));
    }
    items.iter().find_map(|item| match item {
        ClipData::Files(entries) => {
            let paths: Vec<PathBuf> = entries.iter().filter_map(|entry| entry.local_path.clone()).collect();
            (!paths.is_empty()).then_some(Publication::Files(paths))
        }
        _ => None,
    })
}

/// 发布之后剪贴板里会读到的表示。用于内容身份，避免把自己的写入当成外部变化。
fn published_items(publication: &Publication) -> Vec<ClipData> {
    match publication {
        Publication::Image(png) => vec![ClipData::Png(png.clone())],
        Publication::Html { html, alt } => {
            let mut items = vec![ClipData::Html(html.clone())];
            if let Some(alt) = alt {
                items.push(ClipData::Text(alt.clone()));
            }
            items
        }
        Publication::Text(text) => vec![ClipData::Text(text.clone())],
        Publication::Files(paths) => vec![ClipData::Files(file_entries(paths))],
    }
}

/// 把选中的表示写进剪贴板。
fn publish(clipboard: &mut Arboard, publication: &Publication, sensitivity: Sensitivity) -> Result<(), ClipboardError> {
    // `Concealed` 用 KDE 通行的 `x-kde-passwordManagerHint` 表示：桌面剪贴板管理器不应把它写进历史。
    let concealed = sensitivity == Sensitivity::Concealed;
    match publication {
        Publication::Image(png) => {
            let image = png_to_image_data(png)?;
            let mut set = clipboard.set();
            if concealed {
                set = set.exclude_from_history();
            }
            set.image(image).map_err(arboard_error)
        }
        Publication::Html { html, alt } => {
            let mut set = clipboard.set();
            if concealed {
                set = set.exclude_from_history();
            }
            set.html(html.clone(), alt.clone()).map_err(arboard_error)
        }
        Publication::Text(text) => {
            let mut set = clipboard.set();
            if concealed {
                set = set.exclude_from_history();
            }
            set.text(text.clone()).map_err(arboard_error)
        }
        Publication::Files(paths) => {
            let mut set = clipboard.set();
            if concealed {
                set = set.exclude_from_history();
            }
            set.file_list(paths).map_err(arboard_error)
        }
    }
}

/// 读取剪贴板当前持有的表示。探测顺序：图片 → 文件清单 → HTML → 纯文本。
fn read_items(clipboard: &mut Arboard) -> Result<Vec<ClipData>, ClipboardError> {
    if let Ok(image) = clipboard.get().image() {
        return Ok(vec![ClipData::Png(image_data_to_png(&image)?)]);
    }
    if let Ok(paths) = clipboard.get().file_list()
        && !paths.is_empty()
    {
        return Ok(vec![ClipData::Files(file_entries(&paths))]);
    }
    let html = clipboard.get().html().ok();
    let text = clipboard.get().text().ok();
    let mut items = Vec::new();
    if let Some(html) = html {
        items.push(ClipData::Html(html));
    }
    if let Some(text) = text {
        items.push(ClipData::Text(text));
    }
    Ok(items)
}

/// `text/uri-list` 里的路径 → 项目模型的文件清单。顶层条目的名字就是文件名本身。
fn file_entries(paths: &[PathBuf]) -> Vec<FileEntry> {
    paths
        .iter()
        .map(|path| {
            let metadata = std::fs::metadata(path).ok();
            let is_dir = metadata.as_ref().is_some_and(|metadata| metadata.is_dir());
            let size = metadata.as_ref().map(|metadata| metadata.len()).unwrap_or(0);
            let modified_ms = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_millis() as i64);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            FileEntry {
                path: name,
                is_dir,
                size,
                modified_ms,
                local_path: Some(path.clone()),
            }
        })
        .collect()
}

/// 项目的 PNG 字节 → arboard 的 RGBA8 图像。
fn png_to_image_data(png: &[u8]) -> Result<ImageData<'static>, ClipboardError> {
    let (width, height, pixels) =
        png_to_rgba(png).ok_or_else(|| ClipboardError::Platform("解码剪贴板 PNG 失败".to_owned()))?;
    Ok(ImageData {
        width: width as usize,
        height: height as usize,
        bytes: Cow::Owned(pixels),
    })
}

/// arboard 的 RGBA8 图像 → 项目的 PNG 字节。
fn image_data_to_png(image: &ImageData<'_>) -> Result<Vec<u8>, ClipboardError> {
    let buffer = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.as_ref().to_vec())
        .ok_or_else(|| ClipboardError::Platform("剪贴板图像尺寸与像素数据不匹配".to_owned()))?;
    let mut png = Vec::new();
    buffer
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| ClipboardError::Platform(format!("编码剪贴板 PNG 失败：{error}")))?;
    Ok(png)
}

/// PNG 字节 → RGBA8（行优先、自顶向下）。
fn png_to_rgba(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((width, height, rgba.into_raw()))
}

/// 内容身份：对当前内容的规范字节做 blake3。
///
/// 图片按像素（RGBA8）计算，所以同一张图的两种 PNG 编码得到同一个身份；
/// 文本、HTML、文件清单按字节计算。
fn content_key(items: &[ClipData]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"oink/clipboard/linux-content/1");
    for item in items {
        hash_item(&mut hasher, item);
    }
    *hasher.finalize().as_bytes()
}

fn hash_item(hasher: &mut Hasher, item: &ClipData) {
    match item {
        ClipData::Text(text) => {
            hasher.update(b"text\0");
            hasher.update(text.as_bytes());
        }
        ClipData::Html(html) => {
            hasher.update(b"html\0");
            hasher.update(html.as_bytes());
        }
        ClipData::Rtf(bytes) => {
            hasher.update(b"rtf\0");
            hasher.update(bytes);
        }
        ClipData::Png(bytes) => {
            hasher.update(b"png\0");
            match png_to_rgba(bytes) {
                Some((width, height, pixels)) => {
                    hasher.update(&width.to_le_bytes());
                    hasher.update(&height.to_le_bytes());
                    hasher.update(&pixels);
                }
                // 解不开就退回原始字节：同一份坏数据仍然得到同一个身份。
                None => {
                    hasher.update(bytes);
                }
            }
        }
        ClipData::Files(entries) => {
            hasher.update(b"files\0");
            for entry in entries {
                hasher.update(entry.path.as_bytes());
                hasher.update(&[u8::from(entry.is_dir)]);
                hasher.update(&entry.size.to_le_bytes());
            }
        }
        ClipData::Custom { platform, name, bytes } => {
            hasher.update(b"custom\0");
            hasher.update(&[platform_tag(*platform)]);
            hasher.update(name.as_bytes());
            hasher.update(bytes);
        }
        ClipData::Native { platform, item } => {
            hasher.update(b"native\0");
            hasher.update(&[platform_tag(*platform)]);
            hasher.update(&item.id.0.to_le_bytes());
            hasher.update(&(item.representations.len() as u64).to_le_bytes());
            for representation in &item.representations {
                hasher.update(&representation.id.0.to_le_bytes());
                if let RepresentationData::Inline(bytes) = &representation.data {
                    hasher.update(bytes);
                }
            }
        }
        ClipData::FileRoots(nodes) => {
            hasher.update(b"roots\0");
            for node in nodes {
                hasher.update(node.name.as_bytes());
                hasher.update(&[node_kind_tag(&node.kind)]);
                match &node.size {
                    ContentSize::Unknown => {
                        hasher.update(&[0u8]);
                    }
                    ContentSize::Known(size) => {
                        hasher.update(&[1u8]);
                        hasher.update(&size.to_le_bytes());
                    }
                }
            }
        }
    }
}

fn platform_tag(platform: ClipPlatform) -> u8 {
    match platform {
        ClipPlatform::Mac => 0,
        ClipPlatform::Windows => 1,
        ClipPlatform::Linux => 3,
        ClipPlatform::Unknown => 2,
    }
}

fn node_kind_tag(kind: &NodeKind) -> u8 {
    match kind {
        NodeKind::File => 0,
        NodeKind::Directory => 1,
        NodeKind::Symlink(_) => 2,
    }
}

/// arboard 的错误 → 本项目的错误。
fn arboard_error(error: ArboardError) -> ClipboardError {
    match error {
        ArboardError::ClipboardOccupied => ClipboardError::Busy,
        other => ClipboardError::Platform(format!("X11 剪贴板操作失败：{other}")),
    }
}

/// 读取当前内容并计算身份；读取失败时这一轮跳过。
fn current_key(inner: &Inner) -> Option<[u8; 32]> {
    let mut clipboard = inner.clipboard.lock().unwrap_or_else(|error| error.into_inner());
    let items = read_items(&mut clipboard).ok()?;
    Some(content_key(&items))
}

/// 启动监听线程。
fn spawn_watcher(inner: Arc<Inner>) -> Result<JoinHandle<()>, ClipboardError> {
    std::thread::Builder::new()
        .name("oink-clipboard-watch".into())
        .spawn(move || watch_loop(inner))
        .map_err(|error| ClipboardError::Platform(format!("创建剪贴板监听线程失败：{error}")))
}

/// 轮询内容身份；变了就计数并通知所有回调。
fn watch_loop(inner: Arc<Inner>) {
    while !inner.stop.load(Ordering::Acquire) {
        // 暂停期间不轮询；恢复时立刻重新检查一次。
        {
            let mut state = inner.state.lock().unwrap_or_else(|error| error.into_inner());
            while state.paused() && !inner.stop.load(Ordering::Acquire) {
                state = inner.wake.wait(state).unwrap_or_else(|error| error.into_inner());
            }
            drop(state);
        }
        if inner.stop.load(Ordering::Acquire) {
            return;
        }
        let deadline = Instant::now() + POLL_INTERVAL;
        if let Some(key) = current_key(&inner) {
            let changed = {
                let mut state = inner.state.lock().unwrap_or_else(|error| error.into_inner());
                state.observed(key).is_some()
            };
            if changed {
                let handlers = inner.handlers.lock().unwrap_or_else(|error| error.into_inner()).clone();
                for handler in handlers {
                    handler();
                }
            }
        }
        // 睡到下一个轮询时刻，保证两次轮询的间隔不超过 POLL_INTERVAL。
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
    }
}

#[cfg(all(test, debug_assertions))]
// 无 libtest 的构建会去掉 `#[test]` 的函数体，这些辅助函数因此没人调用。
#[allow(dead_code)]
mod tests {
    use super::*;

    /// 造一张 2×1 的 PNG：红、绿两个像素。
    fn sample_png() -> Vec<u8> {
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let mut png = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn publication_prefers_image_then_text_then_files() {
        let png = sample_png();
        let items = vec![
            ClipData::Files(vec![FileEntry {
                path: "a.txt".into(),
                is_dir: false,
                size: 1,
                modified_ms: None,
                local_path: Some(PathBuf::from("/tmp/a.txt")),
            }]),
            ClipData::Text("文本".into()),
            ClipData::Png(png.clone()),
        ];
        assert_eq!(plan_publication(&items), Some(Publication::Image(png)));

        let text_only = vec![ClipData::Text("文本".into())];
        assert_eq!(plan_publication(&text_only), Some(Publication::Text("文本".into())));

        let html_and_text = vec![ClipData::Text("文本".into()), ClipData::Html("<p>文本</p>".into())];
        assert_eq!(
            plan_publication(&html_and_text),
            Some(Publication::Html {
                html: "<p>文本</p>".into(),
                alt: Some("文本".into())
            })
        );

        let files = vec![ClipData::Files(vec![FileEntry {
            path: "a.txt".into(),
            is_dir: false,
            size: 1,
            modified_ms: None,
            local_path: Some(PathBuf::from("/tmp/a.txt")),
        }])];
        assert_eq!(
            plan_publication(&files),
            Some(Publication::Files(vec![PathBuf::from("/tmp/a.txt")]))
        );

        assert_eq!(plan_publication(&[]), None);
    }

    #[test]
    fn published_items_describe_what_a_read_returns() {
        let png = sample_png();
        let publication = Publication::Image(png.clone());
        assert_eq!(published_items(&publication), vec![ClipData::Png(png)]);

        let publication = Publication::Html {
            html: "<p>文本</p>".into(),
            alt: Some("文本".into()),
        };
        assert_eq!(
            published_items(&publication),
            vec![ClipData::Html("<p>文本</p>".into()), ClipData::Text("文本".into())]
        );

        let publication = Publication::Text("文本".into());
        assert_eq!(published_items(&publication), vec![ClipData::Text("文本".into())]);
    }

    #[test]
    fn png_converts_to_rgba8_and_back() {
        let png = sample_png();
        let (width, height, pixels) = png_to_rgba(&png).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, vec![255, 0, 0, 255, 0, 255, 0, 255]);

        let image = png_to_image_data(&png).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.bytes.as_ref(), pixels.as_slice());

        let encoded = image_data_to_png(&image).unwrap();
        let (width, height, decoded) = png_to_rgba(&encoded).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn file_list_becomes_file_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        let entries = file_entries(&[file.clone(), sub.clone()]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "a.txt");
        assert!(!entries[0].is_dir);
        assert_eq!(entries[0].size, 5);
        assert_eq!(entries[0].local_path, Some(file));
        assert_eq!(entries[1].path, "sub");
        assert!(entries[1].is_dir);
        assert_eq!(entries[1].local_path, Some(sub));
    }

    #[test]
    fn content_key_distinguishes_content() {
        let text = vec![ClipData::Text("你好".into())];
        let same = vec![ClipData::Text("你好".into())];
        let other = vec![ClipData::Text("再见".into())];
        assert_eq!(content_key(&text), content_key(&same));
        assert_ne!(content_key(&text), content_key(&other));

        // 同一张图的两种 PNG 编码（这里用同一份字节）得到同一个身份。
        let png = sample_png();
        assert_eq!(
            content_key(&[ClipData::Png(png.clone())]),
            content_key(&[ClipData::Png(png)])
        );

        // 空剪贴板有稳定的身份。
        assert_eq!(content_key(&[]), content_key(&[]));
    }

    #[test]
    fn change_state_counts_only_real_changes() {
        let mut state = ChangeState::default();
        assert_eq!(state.count(), 1);

        let key = content_key(&[ClipData::Text("你好".into())]);
        let other = content_key(&[ClipData::Text("再见".into())]);

        // 基线不产生计数。
        state.baseline(key);
        assert_eq!(state.count(), 1);
        assert_eq!(state.observed(key), None);
        assert_eq!(state.count(), 1);

        // 内容变了：计数加一。
        assert_eq!(state.observed(other), Some(2));
        assert_eq!(state.observed(other), None);
        assert_eq!(state.count(), 2);

        // 本进程发布：计数加一，并把内容记为已观察。
        assert_eq!(state.published(key), 3);
        assert_eq!(state.observed(key), None);
        assert_eq!(state.count(), 3);
    }

    #[test]
    fn change_state_pause_keeps_registrations() {
        let mut state = ChangeState::default();
        assert!(!state.paused());
        state.set_paused(true);
        assert!(state.paused());
        // 暂停标志与内容身份互相独立：恢复后仍能识别变化。
        state.set_paused(false);
        assert!(!state.paused());
        let key = content_key(&[ClipData::Text("你好".into())]);
        assert_eq!(state.observed(key), Some(2));
    }
}
