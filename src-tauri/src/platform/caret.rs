//! Caret (text-cursor) screen position retrieval.
//!
//! Relocated verbatim from `window_mgmt.rs` (Phase 5.5 platform-seam
//! relocation): the Windows arm tries classic Win32 `GetGUIThreadInfo` first,
//! then falls back to UI Automation COM. Other platforms return `None` for now.

/// Screen-space caret position, when it can be resolved.
#[cfg(windows)]
pub fn get_caret_position() -> Option<(i32, i32)> {
    unsafe {
        // 1. Try classic Win32 GetGUIThreadInfo
        if let Some(pos) = get_caret_from_gui_thread_info() {
            return Some(pos);
        }
        // 2. Try modern UI Automation
        if let Some(pos) = get_caret_from_uia() {
            return Some(pos);
        }
    }
    None
}

#[cfg(not(windows))]
pub fn get_caret_position() -> Option<(i32, i32)> {
    None
}

#[cfg(windows)]
unsafe fn get_caret_from_gui_thread_info() -> Option<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::Foundation::POINT;

    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };

    if GetGUIThreadInfo(0, &mut info).is_ok() {
        // `rcCaret` is relative to `hwndCaret`'s client area (which can be a
        // child control of `hwndFocus`); a null `hwndCaret` means the thread
        // has no caret at all. A caret at client x=0 or y=0 is legitimate, so
        // only a degenerate (height-less) rect is rejected.
        if !info.hwndCaret.is_invalid() && info.rcCaret.bottom > info.rcCaret.top {
            let mut pt = POINT {
                x: info.rcCaret.left,
                y: info.rcCaret.top,
            };
            if ClientToScreen(info.hwndCaret, &mut pt).as_bool() {
                return Some((pt.x, pt.y));
            }
        }
    }
    None
}

#[cfg(windows)]
unsafe fn get_caret_from_uia() -> Option<(i32, i32)> {
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern2,
        UIA_TextPattern2Id,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoCreateInstance, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };
    use windows::core::Interface;

    // Best effort COM initialization
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

    let automation: IUIAutomation = CoCreateInstance(
        &CUIAutomation,
        None,
        CLSCTX_INPROC_SERVER,
    ).ok()?;

    let focused: IUIAutomationElement = automation.GetFocusedElement().ok()?;

    let pattern_ptr = focused.GetCurrentPattern(UIA_TextPattern2Id).ok()?;
    let text_pattern2: IUIAutomationTextPattern2 = pattern_ptr.cast().ok()?;

    let mut is_active = windows::Win32::Foundation::BOOL::default();
    let range = text_pattern2.GetCaretRange(&mut is_active).ok()?;

    let rects = range.GetBoundingRectangles().ok()?;
    if rects.is_null() {
        return None;
    }

    // The array holds [left, top, width, height] per rectangle. A degenerate
    // caret range can legitimately return an *empty* (non-null) array, so
    // bound-check before dereferencing. The array is owned by us: destroy it
    // on every path or it leaks each time the bar is shown.
    let mut pos = None;
    let lbound = SafeArrayGetLBound(rects, 1).unwrap_or(0);
    let ubound = SafeArrayGetUBound(rects, 1).unwrap_or(-1);
    if ubound - lbound + 1 >= 4 {
        let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(rects, &mut data_ptr).is_ok() {
            let f64_ptr = data_ptr as *const f64;
            pos = Some((*f64_ptr as i32, (*f64_ptr.add(1)) as i32));
            let _ = SafeArrayUnaccessData(rects);
        }
    }
    let _ = SafeArrayDestroy(rects);

    pos
}
