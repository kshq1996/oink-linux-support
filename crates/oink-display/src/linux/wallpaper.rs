//! Linux：读当前桌面的壁纸，做成缩略图（`crate::wallpaper`）。
//!
//! - 来源：桌面环境自己写的配置，不截屏，也不需要屏幕录制权限。
//!   - KDE Plasma：`$XDG_CONFIG_HOME/plasma-org.kde.plasma.desktop-appletsrc` 里
//!     `[Containments][N][Wallpaper][<插件>][General] Image=file://…`。
//!   - GNOME：`gsettings get org.gnome.desktop.background picture-uri`（深色主题再试
//!     `picture-uri-dark`）。
//! - 一张图铺所有屏：这两个桌面环境都是一张图铺满所有屏幕，所以每块屏按自己的宽高比裁剪同一张图
//!   （Plasma 6 能每屏一张，这里不区分）。
//! - 拿不到静态图片（认不出桌面环境、没有配置、配置指向目录或幻灯片、文件不存在、解码失败）就没有
//!   缩略图，界面上的屏幕还是一片灰。
//! - 壁纸可能是私人照片：本模块不记录路径、内容或任何由它们算出的值。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Once, mpsc};
use std::time::{Duration, Instant, SystemTime};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits, RgbImage};

use crate::wallpaper::{DisplayWallpaper, TARGET_BYTES, Thumbnail, thumbnail_size};
use crate::{ChangeHandler, DisplayInfo};

/// 依次尝试的 JPEG 质量（0..=100）：第一个不超过 [`TARGET_BYTES`] 的结果就用；都超过时用最后一个。
const QUALITIES: [u8; 4] = [72, 55, 40, 28];

/// 轮询桌面环境配置的间隔。
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 读 `gsettings` 的超时：壁纸读不出来也不该把后台线程卡住。
const GSETTINGS_TIMEOUT: Duration = Duration::from_secs(2);

/// 解码时允许的最大内存（壁纸可能是 8K 图）。
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// 当前会话的桌面环境。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Desktop {
    /// KDE Plasma。
    Plasma,
    /// GNOME。
    Gnome,
}

/// 所有屏幕的壁纸缩略图。
///
/// 一张壁纸图铺满所有屏幕，所以图只解码一次，每块屏按自己的宽高比裁剪出缩略图。
pub(super) fn read_all(displays: &[DisplayInfo]) -> Option<Vec<DisplayWallpaper>> {
    let path = source_path()?;
    read_all_from(displays, &path)
}

/// 用指定的壁纸图给所有屏幕做缩略图。
fn read_all_from(displays: &[DisplayInfo], path: &Path) -> Option<Vec<DisplayWallpaper>> {
    let image = decode(path)?;
    let mut out = Vec::with_capacity(displays.len());
    for display in displays {
        let Some((tw, th)) = thumbnail_size(f64::from(display.pixel_width), f64::from(display.pixel_height)) else {
            continue;
        };
        let Some(thumbnail) = render(&image, tw, th) else {
            continue;
        };
        out.push(DisplayWallpaper {
            key: display.key.clone(),
            thumbnail,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// 把图片铺满 `tw × th` 的画面（等比放缩、居中裁剪），编码成 JPEG。
fn render(image: &DynamicImage, tw: u32, th: u32) -> Option<Thumbnail> {
    let filled = image.resize_to_fill(tw, th, FilterType::Triangle);
    let rgb = filled.to_rgb8();
    let mut encoded = None;
    for quality in QUALITIES {
        let Some(bytes) = encode_jpeg(&rgb, quality) else {
            continue;
        };
        let size = bytes.len();
        encoded = Some(bytes);
        if size <= TARGET_BYTES {
            break;
        }
    }
    let jpeg = encoded?;
    Some(Thumbnail {
        width: u16::try_from(tw).ok()?,
        height: u16::try_from(th).ok()?,
        jpeg,
    })
}

/// 编码成 JPEG。
fn encode_jpeg(image: &RgbImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(image)
        .ok()?;
    Some(out)
}

/// 解码壁纸文件。不是静态图片（目录、幻灯片清单、视频）或解码失败时为 `None`。
fn decode(path: &Path) -> Option<DynamicImage> {
    register_jxl_hook();
    let mut reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().ok()
}

/// 把 JPEG XL 的解码钩子注册给 `image`（只注册一次）。
///
/// `image` 自己不认识 JPEG XL，而 Fedora 与 Bazzite 的默认壁纸就是 `.jxl`：
/// 钩子同时登记解码器和文件头，所以读图的地方不用知道格式。
fn register_jxl_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        jxl_oxide::integration::register_image_decoding_hook();
    });
}

/// 壁纸变化的监听器：桌面环境的配置变了就回调。
pub(super) struct Watcher {
    stop: mpsc::Sender<()>,
}

impl Watcher {
    /// 起一个轮询线程。起不来时返回 `None`（没有壁纸变化通知，不影响别的功能）。
    pub(super) fn spawn(on_change: ChangeHandler) -> Option<Self> {
        let (stop, signals) = mpsc::channel();
        std::thread::Builder::new()
            .name("oink-wallpaper".to_string())
            .spawn(move || poll(signals, on_change))
            .ok()?;
        Some(Self { stop })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// 轮询配置文件，变了就通知。
fn poll(signals: mpsc::Receiver<()>, on_change: ChangeHandler) {
    let mut last = stamp();
    loop {
        match signals.recv_timeout(POLL_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = stamp();
        if now != last {
            last = now;
            on_change();
        }
    }
}

/// 判据：桌面环境写壁纸设置的那个文件的修改时间。
fn stamp() -> Option<(PathBuf, SystemTime)> {
    let path = match desktop()? {
        Desktop::Plasma => plasma_config()?,
        Desktop::Gnome => dconf_db()?,
    };
    let modified = fs::metadata(&path).ok()?.modified().ok()?;
    Some((path, modified))
}

// ───────────── 桌面环境 ─────────────

/// 当前会话的桌面环境。认不出时为 `None`（没有壁纸缩略图）。
fn desktop() -> Option<Desktop> {
    let mut gnome = None;
    for name in ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"] {
        let value = std::env::var(name).unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        let upper = value.to_ascii_uppercase();
        if upper.contains("KDE") || upper.contains("PLASMA") {
            return Some(Desktop::Plasma);
        }
        if upper.contains("GNOME") || upper.contains("UNITY") {
            gnome = Some(Desktop::Gnome);
        }
    }
    gnome
}

/// 当前壁纸图片的路径。
fn source_path() -> Option<PathBuf> {
    match desktop()? {
        Desktop::Plasma => plasma_wallpaper(),
        Desktop::Gnome => gnome_wallpaper(),
    }
}

// ───────────── KDE Plasma ─────────────

/// Plasma 的配置文件。
fn plasma_config() -> Option<PathBuf> {
    Some(config_home()?.join("plasma-org.kde.plasma.desktop-appletsrc"))
}

/// Plasma 的当前壁纸图片。
fn plasma_wallpaper() -> Option<PathBuf> {
    plasma_wallpaper_in(&plasma_config()?)
}

/// 配置文件 `config` 里写的当前壁纸图片。
fn plasma_wallpaper_in(config: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(config).ok()?;
    let value = plasma_image(&text)?;
    image_path(&value)
}

/// 从 Plasma 的 appletsrc 里找出静态壁纸插件的图片 URI。
///
/// 形如 `[Containments][26][Wallpaper][org.kde.image][General]` 的组里，`Image=` 是图片的
/// `file://` URI。幻灯片插件（`org.kde.slideshow`）记的是上一次用过的图，不算当前壁纸。
fn plasma_image(config: &str) -> Option<String> {
    const PLUGINS: [&str; 2] = ["org.kde.image", "org.kde.plasma.image"];
    let mut group: Vec<&str> = Vec::new();
    let mut found: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = group_header(line) {
            group = header;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "Image" || !wallpaper_group(&group, &PLUGINS) {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if found.is_none() {
            found = Some(value.to_string());
        }
    }
    found
}

/// 一行是不是组的标题；是的话给出组的各段。
fn group_header(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.split("][").collect())
}

/// 组的路径是不是静态壁纸插件的。
fn wallpaper_group(group: &[&str], plugins: &[&str; 2]) -> bool {
    let Some(position) = group.iter().position(|part| *part == "Wallpaper") else {
        return false;
    };
    let Some(plugin) = group.get(position + 1) else {
        return false;
    };
    plugins.contains(plugin)
}

// ───────────── GNOME ─────────────

/// GNOME 的壁纸设置。
fn gnome_wallpaper() -> Option<PathBuf> {
    for key in ["picture-uri", "picture-uri-dark"] {
        let Some(uri) = gsettings_get("org.gnome.desktop.background", key) else {
            continue;
        };
        let Some(path) = image_path(&uri) else {
            continue;
        };
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// GNOME 的 dconf 数据库：壁纸变了它就变。
fn dconf_db() -> Option<PathBuf> {
    Some(config_home()?.join("dconf").join("user"))
}

/// 读一个 gsettings 键，返回字符串值（去掉 GVariant 的引号）。
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let output = run_gsettings(schema, key)?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let value = match text.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        Some(inner) => inner.to_string(),
        None => text,
    };
    (!value.is_empty()).then_some(value)
}

/// 跑一次 `gsettings get`，最多等 [`GSETTINGS_TIMEOUT`]。
fn run_gsettings(schema: &str, key: &str) -> Option<std::process::Output> {
    let mut child = Command::new("gsettings")
        .args(["get", schema, key])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + GSETTINGS_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    };
    if !status.success() {
        return None;
    }
    child.wait_with_output().ok()
}

// ───────────── 路径 ─────────────

/// 配置文件目录。
fn config_home() -> Option<PathBuf> {
    let explicit = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
    if !explicit.is_empty() {
        return Some(PathBuf::from(explicit));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    (!home.is_empty()).then(|| PathBuf::from(home).join(".config"))
}

/// 壁纸配置里写的图片位置。
///
/// Plasma 5 写的是 `file://` URI，Plasma 6 直接写绝对路径（`/usr/share/wallpapers/…`），
/// 两种都要认。带主机名的（`file://host/path`）不支持。
fn image_path(value: &str) -> Option<PathBuf> {
    if let Some(rest) = value.strip_prefix("file://") {
        if !rest.starts_with('/') {
            return None;
        }
        return Some(PathBuf::from(percent_decode(rest)));
    }
    if value.starts_with('/') {
        return Some(PathBuf::from(value));
    }
    None
}

/// 解码 `file://` URI 里的百分号编码。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 一个十六进制数字字符的值。
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plasma 5 的配置：静态图片插件。
    const PLASMA_5: &str = "\
[Containments][12][Wallpaper][org.kde.image][General]
FillMode=2
Image=file:///usr/share/wallpapers/openSUSE-default.xml
PreviewImage=file:///usr/share/wallpapers/openSUSE-default.xml

[Containments][12][Wallpaper][org.kde.image][General][Effects]
FadeTime=850
";

    /// Plasma 6 的配置：幻灯片插件记的是上一次用过的图，不算当前壁纸。
    const PLASMA_6: &str = "\
[Containments][131][Wallpaper][org.kde.slideshow][General]
Image=file:///var/lib/wallpapers/old.png
";

    #[test]
    fn plasma_image_finds_the_static_plugin() {
        assert_eq!(
            plasma_image(PLASMA_5).as_deref(),
            Some("file:///usr/share/wallpapers/openSUSE-default.xml")
        );
    }

    #[test]
    fn plasma_image_skips_the_slideshow_plugin() {
        // 幻灯片插件排在前面也不能选它：它记的是上一次用过的图。
        let config = format!("{PLASMA_6}\n[Containments][131][Wallpaper][org.kde.image][General]\nImage=file:///a.png");
        assert_eq!(plasma_image(&config).as_deref(), Some("file:///a.png"));
    }

    #[test]
    fn plasma_image_reads_plasma_6_plugin_ids() {
        let config = "[Containments][1][Wallpaper][org.kde.plasma.image][General]\nImage=file:///b.png";
        assert_eq!(plasma_image(config).as_deref(), Some("file:///b.png"));
    }

    #[test]
    fn plasma_image_ignores_other_groups() {
        let config = "\
[Containments][1][General]
Image=file:///not-a-wallpaper.png

[Containments][1][Wallpaper][org.kde.color][General]
Color=#000000
";
        assert_eq!(plasma_image(config), None);
    }

    #[test]
    fn group_header_splits_every_segment() {
        assert_eq!(
            group_header("[Containments][26][Wallpaper][org.kde.image][General]"),
            Some(vec!["Containments", "26", "Wallpaper", "org.kde.image", "General"])
        );
        assert_eq!(group_header("Image=file:///a.png"), None);
    }

    #[test]
    fn image_path_reads_local_files() {
        assert_eq!(
            image_path("file:///usr/share/backgrounds/a.png"),
            Some(PathBuf::from("/usr/share/backgrounds/a.png"))
        );
    }

    #[test]
    fn image_path_reads_the_plain_path_of_plasma_6() {
        // Plasma 6 直接写绝对路径，不写 `file://` URI。
        assert_eq!(
            image_path("/usr/share/wallpapers/convergence.jxl"),
            Some(PathBuf::from("/usr/share/wallpapers/convergence.jxl"))
        );
    }

    #[test]
    fn image_path_decodes_percent_escapes() {
        assert_eq!(
            image_path("file:///usr/share/Plasma%20Solid.png"),
            Some(PathBuf::from("/usr/share/Plasma Solid.png"))
        );
        assert_eq!(
            image_path("file:///usr/share/%E5%9B%BE%E7%89%87.png"),
            Some(PathBuf::from("/usr/share/图片.png"))
        );
    }

    #[test]
    fn image_path_rejects_other_forms() {
        assert_eq!(image_path("file://host/share/a.png"), None);
        assert_eq!(image_path("https://example.invalid/a.png"), None);
        assert_eq!(image_path("relative.png"), None);
    }

    #[test]
    fn gsettings_value_drops_the_quotes() {
        let value = "'file:///usr/share/backgrounds/warty.png'";
        let stripped = value
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
            .unwrap();
        assert_eq!(stripped, "file:///usr/share/backgrounds/warty.png");
    }

    #[test]
    fn render_produces_a_jpeg_of_the_screen_ratio() {
        let source = image::RgbImage::from_pixel(64, 32, image::Rgb([20, 40, 60]));
        let dynamic = DynamicImage::ImageRgb8(source);
        let thumbnail = render(&dynamic, 480, 240).expect("thumbnail");
        assert_eq!((thumbnail.width, thumbnail.height), (480, 240));
        assert_eq!(&thumbnail.jpeg[..3], &[0xff, 0xd8, 0xff]);
    }

    #[test]
    fn render_crops_a_wide_wallpaper_to_a_tall_screen() {
        let source = image::RgbImage::from_pixel(64, 32, image::Rgb([20, 40, 60]));
        let dynamic = DynamicImage::ImageRgb8(source);
        let thumbnail = render(&dynamic, 240, 480).expect("thumbnail");
        assert_eq!((thumbnail.width, thumbnail.height), (240, 480));
    }

    #[test]
    fn percent_decode_keeps_plain_text() {
        assert_eq!(percent_decode("/usr/share/a.png"), "/usr/share/a.png");
        assert_eq!(percent_decode("/usr/share/100%.png"), "/usr/share/100%.png");
    }

    #[test]
    fn the_plasma_wallpaper_becomes_a_thumbnail_per_screen() {
        use oink_foundation::geom::Rect;

        let directory = tempfile::TempDir::new().expect("the temp dir");
        let wallpaper = directory.path().join("wallpaper.png");
        let source = image::RgbImage::from_pixel(64, 32, image::Rgb([10, 20, 30]));
        DynamicImage::ImageRgb8(source)
            .save(&wallpaper)
            .expect("the wallpaper file");
        let config = directory.path().join("plasma-org.kde.plasma.desktop-appletsrc");
        fs::write(
            &config,
            format!(
                "[Containments][1][Wallpaper][org.kde.image][General]\nImage=file://{}\n",
                wallpaper.display()
            ),
        )
        .expect("the plasma config");

        assert_eq!(plasma_wallpaper_in(&config), Some(wallpaper.clone()));

        let display = DisplayInfo {
            key: "DP-1".to_string(),
            name: "DP-1".to_string(),
            bounds: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            pixel_width: 1920,
            pixel_height: 1080,
            reference_extent: None,
            rotation_deg: 0,
            is_primary: true,
            is_builtin: false,
            is_virtual: false,
        };
        let wallpapers = read_all_from(&[display], &wallpaper).expect("the wallpapers");
        assert_eq!(wallpapers.len(), 1);
        assert_eq!(wallpapers[0].key, "DP-1");
        assert_eq!(
            (wallpapers[0].thumbnail.width, wallpapers[0].thumbnail.height),
            (480, 270)
        );
        assert_eq!(&wallpapers[0].thumbnail.jpeg[..3], &[0xff, 0xd8, 0xff]);
    }

    #[test]
    fn a_missing_wallpaper_leaves_the_screens_empty() {
        let directory = tempfile::TempDir::new().expect("the temp dir");
        assert!(read_all_from(&[], &directory.path().join("nothing.png")).is_none());
    }

    #[test]
    fn a_jpeg_xl_wallpaper_is_decoded() {
        // Fedora 与 Bazzite 的默认壁纸是 JPEG XL，`image` 不认识它：
        // 靠 jxl-oxide 的解码钩子读出来。
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wallpaper.jxl");
        let decoded = decode(&path).expect("the JPEG XL wallpaper");
        assert_eq!((decoded.width(), decoded.height()), (64, 32));
        let rgb = decoded.to_rgb8();
        let left = rgb.get_pixel(8, 16).0;
        let right = rgb.get_pixel(56, 16).0;
        assert!(
            left[0] > 170 && left[1] < 90 && left[2] < 90,
            "the left half is red: {left:?}"
        );
        assert!(
            right[2] > 170 && right[0] < 90 && right[1] < 90,
            "the right half is blue: {right:?}"
        );
    }

    #[test]
    fn a_jpeg_xl_wallpaper_becomes_a_thumbnail_per_screen() {
        use oink_foundation::geom::Rect;

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wallpaper.jxl");
        let display = DisplayInfo {
            key: "DP-1".to_string(),
            name: "DP-1".to_string(),
            bounds: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            pixel_width: 1920,
            pixel_height: 1080,
            reference_extent: None,
            rotation_deg: 0,
            is_primary: true,
            is_builtin: false,
            is_virtual: false,
        };
        let wallpapers = read_all_from(&[display], &path).expect("the wallpapers");
        assert_eq!(wallpapers.len(), 1);
        assert_eq!(
            (wallpapers[0].thumbnail.width, wallpapers[0].thumbnail.height),
            (480, 270)
        );
        assert_eq!(&wallpapers[0].thumbnail.jpeg[..3], &[0xff, 0xd8, 0xff]);
    }

    #[test]
    fn the_plasma_6_wallpaper_becomes_a_thumbnail() {
        // Plasma 6 的配置写绝对路径，图片是 JPEG XL：这条就是桌面上真正发生的事。
        use oink_foundation::geom::Rect;

        let directory = tempfile::TempDir::new().expect("the temp dir");
        let wallpaper = directory.path().join("convergence.jxl");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wallpaper.jxl"),
            &wallpaper,
        )
        .expect("the wallpaper file");
        let config = directory.path().join("plasma-org.kde.plasma.desktop-appletsrc");
        fs::write(
            &config,
            format!(
                "[Containments][1][Wallpaper][org.kde.image][General]\nImage={}\n",
                wallpaper.display()
            ),
        )
        .expect("the plasma config");
        assert_eq!(plasma_wallpaper_in(&config), Some(wallpaper.clone()));

        let display = DisplayInfo {
            key: "eDP-1".to_string(),
            name: "eDP-1".to_string(),
            bounds: Rect::new(0.0, 0.0, 1920.0, 1200.0),
            pixel_width: 1920,
            pixel_height: 1200,
            reference_extent: None,
            rotation_deg: 0,
            is_primary: true,
            is_builtin: false,
            is_virtual: false,
        };
        let wallpapers = read_all_from(&[display], &wallpaper).expect("the wallpapers");
        assert_eq!(wallpapers.len(), 1);
        assert_eq!(wallpapers[0].key, "eDP-1");
        assert_eq!(
            (wallpapers[0].thumbnail.width, wallpapers[0].thumbnail.height),
            (480, 300)
        );
        assert_eq!(&wallpapers[0].thumbnail.jpeg[..3], &[0xff, 0xd8, 0xff]);
    }
}
