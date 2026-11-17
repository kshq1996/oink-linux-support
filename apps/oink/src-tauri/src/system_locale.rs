//! Read the user's UI language independently of the embedded browser profile.
#[cfg(any(target_os = "macos", test))]
use oink_agent::config::UiLanguage;

#[cfg(target_os = "macos")]
pub fn preferred_languages() -> Vec<String> {
    objc2_foundation::NSLocale::preferredLanguages()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(windows)]
pub fn preferred_languages() -> Vec<String> {
    use windows::{
        Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
        core::PWSTR,
    };
    let read = || -> windows::core::Result<Vec<String>> {
        let (mut count, mut length) = (0, 0);
        // Windows returns an ordered MULTI_SZ. Query its size before providing
        // storage; WebView2's profile language can remain English on Chinese OSes.
        // SAFETY: the sizing call writes only the two valid scalar outputs.
        unsafe {
            GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length)?;
        }
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: the writable UTF-16 buffer has the capacity just returned by
        // the API; length supplies that capacity and receives the used size.
        unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut length,
            )?;
        }
        Ok(buffer[..length as usize]
            .split(|c| *c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect())
    };
    read().unwrap_or_else(|error| {
        tracing::warn!(%error, "preferred UI languages unavailable");
        Vec::new()
    })
}

/// The desktop's language list lives in the environment: LANGUAGE holds an
/// ordered list of languages, and the locale categories hold one value each.
#[cfg(target_os = "linux")]
pub fn preferred_languages() -> Vec<String> {
    let mut values: Vec<String> = std::env::var("LANGUAGE")
        .unwrap_or_default()
        .split(':')
        .map(str::to_owned)
        .collect();
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            values.push(value);
        }
    }
    let mut languages: Vec<String> = Vec::new();
    for value in values {
        let Some(tag) = tag(&value) else {
            continue;
        };
        if !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    if languages.is_empty() {
        // The C and POSIX locales name no language: the interface is English.
        languages.push("en-US".to_owned());
    }
    languages
}

/// A POSIX locale names a language, a territory and a codeset; the product
/// reads BCP 47 tags.
#[cfg(target_os = "linux")]
fn tag(value: &str) -> Option<String> {
    let value = value.split(['.', '@']).next().unwrap_or(value).trim();
    if value.is_empty() || value.eq_ignore_ascii_case("C") || value.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    Some(value.replace('_', "-"))
}

#[cfg(any(target_os = "macos", test))]
pub fn auto_language() -> UiLanguage {
    UiLanguage::Auto.resolve(preferred_languages().first().map(String::as_str).unwrap_or(""))
}

pub fn initialization_script() -> String {
    // Initialization scripts run before the frontend imports i18n, avoiding an
    // English first render or a second asynchronous language-setting command.
    format!(
        "Object.defineProperty(window, '__OINK_SYSTEM_LANGUAGES__', {{value: {}}});",
        serde_json::to_string(&preferred_languages()).expect("language strings are JSON")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_preference_list_is_available_before_frontend_startup() {
        let languages = super::preferred_languages();
        assert!(!languages.is_empty());
        assert!(super::initialization_script().contains(&serde_json::to_string(&languages).unwrap()));
        assert_eq!(
            super::auto_language(),
            oink_agent::config::UiLanguage::Auto.resolve(&languages[0])
        );
    }
}
