//! Linux：用户的 XDG autostart 项（`$XDG_CONFIG_HOME/autostart/oink.desktop`）。
//!
//! - **XDG 要求**：freedesktop.org 规定自启动项放在 `$XDG_CONFIG_HOME/autostart` 下；
//!   `XDG_CONFIG_HOME` 未设置（或不是绝对路径）时按规范退回 `$HOME/.config/autostart`。
//!   本 crate 只负责用户级的一项（`~/.config/autostart/oink.desktop`），不写系统级的
//!   `/etc/xdg/autostart`：那个目录属于管理员，普通用户改不了。
//! - **各家桌面不一样**：GNOME 会把这一项列在「启动应用程序」里并允许用户关掉
//!   （关掉后文件里会留下 `X-GNOME-Autostart-enabled=false` 或 `Hidden=true`）；KDE 只是把它
//!   当成一个普通的 `.desktop` 文件；XFCE 与其它轻量桌面可能根本不读这个目录。
//!   因此这里**以文件内容为准**，而不是猜桌面环境：文件在且没被关掉就是「已注册」，
//!   文件在但被关掉就是「需要用户重新允许」，文件不在就是「未注册」。
//! - **没有统一的设置页**：Linux 上没有一个跨桌面通用的「登录项」设置页，
//!   [`open_system_settings`] 因此返回 `unsupported_platform`，不去猜一个可能不存在的命令。
//! - 只有界面程序（可执行文件名 `oink`）能登记登录项；命令行工具与后台进程不行。

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::{AUTOSTART_ARG, LaunchAtLoginProblem, LaunchAtLoginState, LaunchAtLoginStatus};

/// 用户的自启动项文件名。
const DESKTOP_FILE_NAME: &str = "oink.desktop";
/// 界面程序的可执行文件名：只有它能登记为登录项（后台是 `oink-backend`，代理是 `oink-agent`）。
const UI_EXE: &str = "oink";
/// desktop 文件里描述这一项的那一组。
const DESKTOP_ENTRY_GROUP: &str = "Desktop Entry";

/// 登录项文件里的错误只留在本地日志里，界面看到的是固定的问题代码。
fn describe(operation: &str, error: io::Error) -> String {
    tracing::warn!(operation, error = %error, "login item operation failed");
    "login_item_system_unavailable".into()
}

/// 当前程序的路径；不是界面程序时返回给用户看的原因。
fn ui_exe() -> Result<PathBuf, LaunchAtLoginProblem> {
    let exe = std::env::current_exe().map_err(|_| LaunchAtLoginProblem::SystemUnavailable)?;
    let is_ui = exe.file_name().is_some_and(|name| name.to_string_lossy() == UI_EXE);
    if is_ui {
        Ok(exe)
    } else {
        Err(LaunchAtLoginProblem::MainAppRequired)
    }
}

/// 用户配置目录：`$XDG_CONFIG_HOME`（必须是绝对路径），否则 `$HOME/.config`。
fn config_home() -> Result<PathBuf, String> {
    config_home_from(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))
}

/// [`config_home`] 的纯函数版本：把两个环境变量值直接传进来，便于测试。
fn config_home_from(config: Option<OsString>, home: Option<OsString>) -> Result<PathBuf, String> {
    if let Some(dir) = config.filter(|value| !value.is_empty()) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Ok(dir);
        }
    }
    let home = home.ok_or_else(|| "home directory is unknown".to_owned())?;
    Ok(PathBuf::from(home).join(".config"))
}

/// 自启动项目录：用户配置目录下的 `autostart`。
fn autostart_dir(config_home: &Path) -> PathBuf {
    config_home.join("autostart")
}

/// 自启动项文件：`~/.config/autostart/oink.desktop`。
fn entry_path(config_home: &Path) -> PathBuf {
    autostart_dir(config_home).join(DESKTOP_FILE_NAME)
}

/// Exec 里的程序路径：按 freedesktop.org 的引号规则转义（`%` 是字段码，写成 `%%`）。
fn exec_argument(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '`' => out.push_str("\\`"),
            '$' => out.push_str("\\$"),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("%%"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// 写进自启动项的 desktop 文件内容。
///
/// `Exec=` 带 [`AUTOSTART_ARG`]：登录时桌面环境启动界面程序，界面程序认出这个参数后
/// 用带 `OINK_LAUNCHED_AT_LOGIN` 标记的方式重启常驻进程（见 [`crate::login_command`]），
/// 这样「本次是否由登录项启动」判断得住。
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Oink\nExec={} {AUTOSTART_ARG}\nIcon=oink\nTerminal=false\nCategories=Utility;RemoteAccess;\nX-GNOME-Autostart-enabled=true\nHidden=false\n",
        exec_argument(&exe.to_string_lossy())
    )
}

/// desktop 文件里我们关心的那几个键。
#[derive(Debug, Default, PartialEq, Eq)]
struct DesktopEntry {
    exec: Option<String>,
    gnome_autostart_enabled: Option<bool>,
    hidden: Option<bool>,
}

/// 读 desktop 文件。只看 `[Desktop Entry]` 这一组里的键；别的组（`[Desktop Action …]`）不算。
fn parse_desktop_entry(text: &str) -> DesktopEntry {
    let mut entry = DesktopEntry::default();
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            in_group = group.trim() == DESKTOP_ENTRY_GROUP;
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Exec" => entry.exec = Some(value.trim().to_owned()),
            "X-GNOME-Autostart-enabled" => entry.gnome_autostart_enabled = parse_bool(value.trim()),
            "Hidden" => entry.hidden = parse_bool(value.trim()),
            _ => {}
        }
    }
    entry
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Exec 行按 shell 的引号规则拆成字段。
fn exec_tokens(exec: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut has_token = false;
    for ch in exec.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            has_token = true;
            continue;
        }
        match ch {
            '\\' if in_quotes => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            ch => {
                current.push(ch);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(current);
    }
    tokens
}

/// Exec 行里的程序是不是当前这个界面程序。
fn same_executable(program: &str, exe: &Path) -> bool {
    let program = Path::new(program);
    if program == exe {
        return true;
    }
    match (fs::canonicalize(program), fs::canonicalize(exe)) {
        // 安装后的 `/usr/bin/oink` 是指向 `/usr/lib/oink/oink` 的符号链接，比较真实路径。
        (Ok(program), Ok(exe)) => program == exe,
        _ => false,
    }
}

/// 由自启动项的内容得出状态。`text` 是文件内容；文件不存在时是 `None`。
fn evaluate(text: Option<&str>, exe: &Path) -> LaunchAtLoginStatus {
    let Some(text) = text else {
        return LaunchAtLoginStatus::new(LaunchAtLoginState::Disabled, None);
    };
    let entry = parse_desktop_entry(text);
    let Some(exec) = entry.exec.as_deref() else {
        return LaunchAtLoginStatus::new(
            LaunchAtLoginState::Disabled,
            Some(LaunchAtLoginProblem::InvalidRunEntry),
        );
    };
    let tokens = exec_tokens(exec);
    let program: &str = tokens.first().map_or(exec, |program| program.as_str());
    let arguments: &[String] = tokens.get(1..).unwrap_or_default();
    if !same_executable(program, exe) {
        return LaunchAtLoginStatus::new(
            LaunchAtLoginState::Disabled,
            Some(LaunchAtLoginProblem::DifferentInstallation),
        );
    }
    if !arguments.iter().map(String::as_str).eq([AUTOSTART_ARG]) {
        return LaunchAtLoginStatus::new(
            LaunchAtLoginState::Disabled,
            Some(LaunchAtLoginProblem::InvalidRunEntry),
        );
    }
    // 桌面环境把这一项关掉时，文件还在，但会被禁用。
    if entry.hidden == Some(true) || entry.gnome_autostart_enabled == Some(false) {
        return LaunchAtLoginStatus::new(
            LaunchAtLoginState::RequiresApproval,
            Some(LaunchAtLoginProblem::ApprovalRequired),
        );
    }
    LaunchAtLoginStatus::new(LaunchAtLoginState::Enabled, None)
}

/// 原子地写出自启动项：先写同目录下的临时文件，再改名。
fn write_entry(dir: &Path, text: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| describe("create_directory", error))?;
    let target = dir.join(DESKTOP_FILE_NAME);
    let mut last_error = None;
    for attempt in 0..8u32 {
        let temporary = dir.join(format!(".{DESKTOP_FILE_NAME}.tmp{}.{attempt}", std::process::id()));
        match write_temporary(&temporary, text) {
            Ok(()) => {
                return match fs::rename(&temporary, &target) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        let _ = fs::remove_file(&temporary);
                        Err(describe("rename", error))
                    }
                };
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                last_error = Some(error);
            }
        }
    }
    Err(last_error.map_or_else(|| "login_item_write_failed".into(), |error| describe("write", error)))
}

/// 写临时文件（同目录、同文件系统，改名才是原子的）。已存在时失败，换一个名字重试。
fn write_temporary(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

/// 删掉自启动项；本来就没有也算成功。
fn remove_entry(dir: &Path) -> Result<(), String> {
    let target = dir.join(DESKTOP_FILE_NAME);
    match fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(describe("remove", error)),
    }
}

/// 自启动项的当前状态。
pub(crate) fn status() -> LaunchAtLoginStatus {
    let exe = match ui_exe() {
        Ok(exe) => exe,
        Err(problem) => return LaunchAtLoginStatus::unavailable(problem),
    };
    let config_home = match config_home() {
        Ok(config_home) => config_home,
        Err(_) => return LaunchAtLoginStatus::unavailable(LaunchAtLoginProblem::SystemUnavailable),
    };
    let path = entry_path(&config_home);
    match fs::read_to_string(&path) {
        Ok(text) => evaluate(Some(&text), &exe),
        Err(error) if error.kind() == io::ErrorKind::NotFound => evaluate(None, &exe),
        Err(error) => {
            let _ = describe("read", error);
            LaunchAtLoginStatus::unavailable(LaunchAtLoginProblem::SystemUnavailable)
        }
    }
}

/// 登记或注销自启动项，返回系统里的状态。
pub(crate) fn set_enabled(enabled: bool) -> Result<LaunchAtLoginStatus, String> {
    let exe = ui_exe().map_err(|problem| problem.code().to_owned())?;
    let config_home = config_home()?;
    let dir = autostart_dir(&config_home);
    if enabled {
        write_entry(&dir, &desktop_entry(&exe))?;
    } else {
        remove_entry(&dir)?;
    }
    Ok(status())
}

/// Linux 上没有跨桌面通用的「登录项」设置页。
///
/// GNOME 的「启动应用程序」、KDE 的「系统设置 → 启动」、XFCE 的「会话与启动」各不相同，
/// 没有一个所有桌面都有的入口可以打开。与其猜一个可能不存在的命令，不如明确告诉调用方
/// 这个平台不支持打开设置页。
pub(crate) fn open_system_settings() -> Result<(), String> {
    Err("unsupported_platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 测试用的临时目录：放在系统临时目录里，测试结束就删掉。
    /// 不读环境变量，因此绝不会碰到真实的用户主目录。
    #[derive(Debug)]
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oink-autostart-{tag}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temporary directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    const EXE: &str = "/opt/oink/oink";

    #[test]
    fn desktop_entry_has_the_keys_a_login_item_needs() {
        let entry = desktop_entry(Path::new(EXE));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        for key in [
            "Type=Application",
            "Name=Oink",
            "Icon=oink",
            "Terminal=false",
            "X-GNOME-Autostart-enabled=true",
            "Hidden=false",
        ] {
            assert!(entry.contains(&format!("\n{key}\n")), "missing {key} in {entry}");
        }
        assert!(
            entry.contains(&format!("\nExec=\"{EXE}\" {AUTOSTART_ARG}\n")),
            "{entry}"
        );
        assert!(entry.ends_with('\n'));
    }

    #[test]
    fn desktop_entry_quotes_paths_with_spaces_and_escapes_percent() {
        let entry = desktop_entry(Path::new("/opt/Oink Dir/100% oink"));
        assert!(
            entry.contains(r#"Exec="/opt/Oink Dir/100%% oink" --autostart"#),
            "{entry}"
        );
    }

    #[test]
    fn exec_tokens_split_quoted_arguments() {
        assert_eq!(
            exec_tokens(r#""/opt/oink/oink" --autostart"#),
            vec!["/opt/oink/oink", "--autostart"]
        );
        assert_eq!(
            exec_tokens("/opt/oink/oink --autostart"),
            vec!["/opt/oink/oink", "--autostart"]
        );
        assert_eq!(exec_tokens(r#""/opt/oink/oink""#), vec!["/opt/oink/oink"]);
    }

    #[test]
    fn writing_the_entry_is_atomic_and_leaves_no_temporary_file() {
        let directory = TempDir::new("atomic");
        let autostart = autostart_dir(directory.path());
        let text = desktop_entry(Path::new(EXE));
        write_entry(&autostart, &text).expect("first write");
        write_entry(&autostart, &text).expect("second write");
        assert_eq!(
            fs::read_to_string(autostart.join(DESKTOP_FILE_NAME)).expect("entry"),
            text
        );
        let names: Vec<String> = fs::read_dir(&autostart)
            .expect("autostart directory")
            .map(|entry| {
                entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec![DESKTOP_FILE_NAME.to_owned()]);
    }

    #[test]
    fn writing_the_entry_creates_the_autostart_directory() {
        let directory = TempDir::new("mkdir");
        let autostart = autostart_dir(&directory.path().join("config"));
        assert!(!autostart.exists());
        write_entry(&autostart, &desktop_entry(Path::new(EXE))).expect("write");
        assert!(autostart.join(DESKTOP_FILE_NAME).is_file());
    }

    #[test]
    fn removing_a_missing_entry_succeeds() {
        let directory = TempDir::new("remove");
        let autostart = autostart_dir(directory.path());
        remove_entry(&autostart).expect("removing an absent entry");
        fs::create_dir_all(&autostart).expect("autostart directory");
        write_entry(&autostart, &desktop_entry(Path::new(EXE))).expect("write");
        remove_entry(&autostart).expect("removing an entry");
        assert!(!autostart.join(DESKTOP_FILE_NAME).exists());
    }

    #[test]
    fn a_missing_entry_is_disabled() {
        let status = evaluate(None, Path::new(EXE));
        assert_eq!(status.state, LaunchAtLoginState::Disabled);
        assert_eq!(status.problem, None);
    }

    #[test]
    fn a_registered_entry_is_enabled() {
        let status = evaluate(Some(&desktop_entry(Path::new(EXE))), Path::new(EXE));
        assert_eq!(status.state, LaunchAtLoginState::Enabled);
        assert_eq!(status.problem, None);
    }

    #[test]
    fn an_entry_disabled_by_the_desktop_requires_approval() {
        for hidden in ["Hidden=true", "X-GNOME-Autostart-enabled=false"] {
            let text = desktop_entry(Path::new(EXE)).replace("Hidden=false\n", &format!("{hidden}\n"));
            let status = evaluate(Some(&text), Path::new(EXE));
            assert_eq!(status.state, LaunchAtLoginState::RequiresApproval, "{hidden}");
            assert_eq!(status.problem, Some(LaunchAtLoginProblem::ApprovalRequired), "{hidden}");
        }
    }

    #[test]
    fn an_entry_for_another_installation_is_disabled() {
        let text = desktop_entry(Path::new("/other/oink"));
        let status = evaluate(Some(&text), Path::new(EXE));
        assert_eq!(status.state, LaunchAtLoginState::Disabled);
        assert_eq!(status.problem, Some(LaunchAtLoginProblem::DifferentInstallation));
    }

    #[test]
    fn an_entry_without_the_autostart_argument_is_invalid() {
        let text = desktop_entry(Path::new(EXE)).replace(&format!(" {AUTOSTART_ARG}\n"), "\n");
        let status = evaluate(Some(&text), Path::new(EXE));
        assert_eq!(status.state, LaunchAtLoginState::Disabled);
        assert_eq!(status.problem, Some(LaunchAtLoginProblem::InvalidRunEntry));
    }

    #[test]
    fn an_entry_without_exec_is_invalid() {
        let text = desktop_entry(Path::new(EXE)).replace(&format!("Exec=\"{EXE}\" {AUTOSTART_ARG}\n"), "");
        let status = evaluate(Some(&text), Path::new(EXE));
        assert_eq!(status.state, LaunchAtLoginState::Disabled);
        assert_eq!(status.problem, Some(LaunchAtLoginProblem::InvalidRunEntry));
    }

    #[test]
    fn the_configuration_directory_follows_the_xdg_rule() {
        assert_eq!(
            config_home_from(Some(OsString::from("/xdg/config")), Some(OsString::from("/home/user"))).unwrap(),
            PathBuf::from("/xdg/config")
        );
        assert_eq!(
            config_home_from(None, Some(OsString::from("/home/user"))).unwrap(),
            PathBuf::from("/home/user/.config")
        );
        // 相对路径按规范忽略，退回 `$HOME/.config`。
        assert_eq!(
            config_home_from(Some(OsString::from("relative")), Some(OsString::from("/home/user"))).unwrap(),
            PathBuf::from("/home/user/.config")
        );
        assert!(config_home_from(None, None).is_err());
        assert_eq!(
            autostart_dir(Path::new("/home/user/.config")),
            PathBuf::from("/home/user/.config/autostart")
        );
        assert_eq!(
            entry_path(Path::new("/home/user/.config")),
            PathBuf::from("/home/user/.config/autostart/oink.desktop")
        );
    }

    #[test]
    fn the_settings_page_is_not_supported_on_linux() {
        assert_eq!(open_system_settings().unwrap_err(), "unsupported_platform");
    }
}
