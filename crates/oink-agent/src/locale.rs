//! The system's preferred UI language.
//!
//! macOS and Windows expose the preference through their locale APIs; Linux
//! keeps it in the POSIX locale environment, where `LC_ALL` overrides
//! `LC_MESSAGES`, which overrides `LANG`.

/// The system's first preferred language, as a language tag.
///
/// An empty string means "the system did not say", which the UI language
/// resolver treats as English.
pub(crate) fn system_language() -> String {
    #[cfg(target_os = "macos")]
    {
        objc2_foundation::NSLocale::preferredLanguages()
            .firstObject()
            .map_or_else(String::new, |s| s.to_string())
    }
    #[cfg(windows)]
    {
        use windows::{
            Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
            core::PWSTR,
        };
        let (mut count, mut length) = (0, 0);
        // SAFETY: sizing query writes only count/length and does not access a buffer.
        unsafe {
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length).is_err() {
                return String::new();
            }
        }
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: UTF-16 output buffer has the capacity reported by the sizing call.
        unsafe {
            if GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut length,
            )
            .is_err()
            {
                return String::new();
            }
        }
        String::from_utf16_lossy(buffer.split(|ch| *ch == 0).next().unwrap_or(&[]))
    }
    #[cfg(target_os = "linux")]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        String::new()
    }
}
