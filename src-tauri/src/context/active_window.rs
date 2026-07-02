//! Resolve the foreground window into a process name, title, and `AppCategory`.
//!
//! On Windows: `GetForegroundWindow`/the captured HWND → `GetWindowTextW`
//! (title) + `GetWindowThreadProcessId` → `OpenProcess` →
//! `QueryFullProcessImageNameW` (executable name). `classify` then maps the
//! process (with browser title/URL heuristics) onto a coarse category that the
//! polish prompt and the Styles page key off of.

/// Coarse classification of the focused app, used to pick a Flow Style and to
/// tag history rows. The `as_str`/`from_str` forms are the stable strings
/// persisted in the DB (`transcripts.app_category`, `flow_styles.app_category`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Email,
    WorkMsg,
    PersonalMsg,
    Code,
    Other,
}

impl AppCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            AppCategory::Email => "email",
            AppCategory::WorkMsg => "workmsg",
            AppCategory::PersonalMsg => "personalmsg",
            AppCategory::Code => "code",
            AppCategory::Other => "other",
        }
    }
}

/// The focused app at record start. `process` is the bare executable name
/// (e.g. `chrome.exe`); `title` is the window caption.
#[derive(Debug, Clone)]
pub struct AppContext {
    pub process: String,
    pub title: String,
    pub category: AppCategory,
}

impl AppContext {
    /// Fallback when nothing could be resolved (non-Windows, or a failed probe).
    pub fn unknown() -> Self {
        Self {
            process: String::new(),
            title: String::new(),
            category: AppCategory::Other,
        }
    }
}

/// Map a process name + window title to a category. Process matches win first;
/// for browsers (no useful process signal) we fall back to title/URL keywords.
pub fn classify(process: &str, title: &str) -> AppCategory {
    let proc = process.to_ascii_lowercase();
    let title_l = title.to_ascii_lowercase();

    // Direct desktop-app matches.
    const EMAIL: &[&str] = &[
        "outlook.exe",
        "thunderbird.exe",
        "mailspring.exe",
        "em client",
    ];
    const WORK_MSG: &[&str] = &["slack.exe", "teams.exe", "ms-teams.exe", "msteams.exe"];
    const PERSONAL_MSG: &[&str] = &[
        "whatsapp.exe",
        "telegram.exe",
        "discord.exe",
        "signal.exe",
        "messenger.exe",
    ];
    const CODE: &[&str] = &[
        "code.exe",
        "cursor.exe",
        "devenv.exe",
        "idea64.exe",
        "pycharm64.exe",
        "webstorm64.exe",
        "goland64.exe",
        "rider64.exe",
        "clion64.exe",
        "sublime_text.exe",
        "windowsterminal.exe",
        "wt.exe",
        "powershell.exe",
        "cmd.exe",
        "alacritty.exe",
    ];

    if EMAIL.iter().any(|p| proc == *p) {
        return AppCategory::Email;
    }
    if WORK_MSG.iter().any(|p| proc == *p) {
        return AppCategory::WorkMsg;
    }
    if PERSONAL_MSG.iter().any(|p| proc == *p) {
        return AppCategory::PersonalMsg;
    }
    if CODE.iter().any(|p| proc == *p) {
        return AppCategory::Code;
    }

    // Browsers: infer from the title (which usually carries the site name/URL).
    const BROWSERS: &[&str] = &[
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "brave.exe",
        "opera.exe",
        "arc.exe",
        "vivaldi.exe",
    ];
    if BROWSERS.iter().any(|p| proc == *p) {
        return classify_browser_title(&title_l);
    }

    AppCategory::Other
}

/// Browser title/URL heuristics: pick a category from well-known site names.
fn classify_browser_title(title: &str) -> AppCategory {
    let contains = |needles: &[&str]| needles.iter().any(|n| title.contains(n));

    if contains(&[
        "gmail",
        "outlook",
        "proton mail",
        "protonmail",
        "yahoo mail",
    ]) {
        AppCategory::Email
    } else if contains(&["slack", "microsoft teams", "google chat"]) {
        AppCategory::WorkMsg
    } else if contains(&[
        "whatsapp",
        "telegram",
        "messenger",
        "discord",
        "signal",
        "instagram",
    ]) {
        AppCategory::PersonalMsg
    } else if contains(&[
        "github",
        "gitlab",
        "stack overflow",
        "stackoverflow",
        "codepen",
        "codesandbox",
        "localhost",
    ]) {
        AppCategory::Code
    } else {
        AppCategory::Other
    }
}

/// Resolve the given foreground window into an `AppContext`.
#[cfg(windows)]
pub fn resolve(hwnd: windows::Win32::Foundation::HWND) -> AppContext {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    if hwnd.0.is_null() {
        return AppContext::unknown();
    }

    unsafe {
        // Window title.
        let len = GetWindowTextLengthW(hwnd);
        let title = if len > 0 {
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        };

        // Owning process id → executable path → bare file name.
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let mut process = String::new();
        if pid != 0 {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = vec![0u16; MAX_PATH as usize];
                let mut size = buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    PWSTR(buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    let full = String::from_utf16_lossy(&buf[..size as usize]);
                    process = full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string();
                }
                let _ = CloseHandle(handle);
            }
        }

        let category = classify(&process, &title);
        AppContext {
            process,
            title,
            category,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_desktop_apps() {
        assert_eq!(classify("OUTLOOK.EXE", ""), AppCategory::Email);
        assert_eq!(classify("slack.exe", ""), AppCategory::WorkMsg);
        assert_eq!(classify("WhatsApp.exe", ""), AppCategory::PersonalMsg);
        assert_eq!(classify("Code.exe", "main.rs"), AppCategory::Code);
        assert_eq!(classify("explorer.exe", ""), AppCategory::Other);
    }

    #[test]
    fn classifies_browser_titles() {
        assert_eq!(
            classify("chrome.exe", "Inbox (3) - me@gmail.com - Gmail"),
            AppCategory::Email
        );
        assert_eq!(
            classify("msedge.exe", "general | Acme - Slack"),
            AppCategory::WorkMsg
        );
        assert_eq!(
            classify("firefox.exe", "WhatsApp"),
            AppCategory::PersonalMsg
        );
        assert_eq!(
            classify("chrome.exe", "eve/pipeline.rs at main · me/eve · GitHub"),
            AppCategory::Code
        );
        assert_eq!(
            classify("chrome.exe", "Some Random Blog Post"),
            AppCategory::Other
        );
    }

    #[test]
    fn category_strings_are_stable() {
        assert_eq!(AppCategory::Email.as_str(), "email");
        assert_eq!(AppCategory::WorkMsg.as_str(), "workmsg");
        assert_eq!(AppCategory::PersonalMsg.as_str(), "personalmsg");
        assert_eq!(AppCategory::Code.as_str(), "code");
        assert_eq!(AppCategory::Other.as_str(), "other");
    }
}
