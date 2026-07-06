//! Phase 3: focused-app context on Linux/X11 - the process (comm) name plus the
//! window title, mapped to an `AppCategory` for Flow Styles and history
//! attribution (the Linux analogue of `active_window::resolve`).
//!
//! `x11.rs` reads the raw signals off the X server (`_NET_WM_PID`,
//! `_NET_WM_NAME`) and hands them here; this module owns only the
//! server-independent part - resolving a pid to its `/proc/<pid>/comm` name and
//! classifying - so it carries no X11 dependency and stays trivially testable.

use crate::context::active_window::{classify, AppContext};

/// Build an `AppContext` from the pid + title already read off the active X
/// window. `process` is the pid's `/proc/<pid>/comm` name (e.g. `code`,
/// `keepassxc`), which the additive Linux entries in `classify` match; `title`
/// is the `_NET_WM_NAME`/`WM_NAME` caption. Falls back to unknown when the pid
/// couldn't be resolved.
pub fn resolve(pid: Option<u32>, title: String) -> AppContext {
    let process = pid.and_then(comm_name).unwrap_or_default();
    let category = classify(&process, &title);
    AppContext {
        process,
        title,
        category,
    }
}

/// The bare command name of a process from `/proc/<pid>/comm`, trimmed of the
/// trailing newline the kernel appends. `None` when the process is gone or the
/// file can't be read. Note the kernel truncates `comm` to 15 characters, so the
/// Linux match lists in `classify` use the truncated forms where relevant.
fn comm_name(pid: u32) -> Option<String> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = raw.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
