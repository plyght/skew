use crate::{Rect, Result, Window, WindowId};
use log::{debug, warn};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

// Core Foundation types
type CFTypeRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *const c_void;
type CFIndex = isize;

// Core Graphics window list options
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

// External C functions from Core Graphics and Core Foundation
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;

    // Core Foundation Array functions
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);

    // Core Foundation Dictionary functions
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFStringRef) -> CFTypeRef;

    // Core Foundation String functions
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetLength(string: CFStringRef) -> CFIndex;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> bool;

    // Core Foundation Number functions
    fn CFNumberGetValue(number: CFNumberRef, number_type: c_int, value_ptr: *mut c_void) -> bool;
}

// Core Foundation String encoding
const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

// Core Foundation Number types
const K_CF_NUMBER_DOUBLE_TYPE: c_int = 13;

pub struct CGWindowInfo;

impl CGWindowInfo {
    pub fn get_all_windows() -> Result<Vec<Window>> {
        debug!("Enumerating windows via CGWindowListCopyWindowInfo");

        let mut windows = Vec::new();

        unsafe {
            let window_list_info = CGWindowListCopyWindowInfo(
                K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
                0, // relative_to_window (0 means all windows)
            );

            if window_list_info.is_null() {
                warn!("Failed to get window list from CGWindowListCopyWindowInfo");
                return Ok(windows);
            }

            let count = CFArrayGetCount(window_list_info);

            for i in 0..count {
                let window_dict = CFArrayGetValueAtIndex(window_list_info, i) as CFDictionaryRef;
                if !window_dict.is_null() {
                    if let Some(window) = Self::parse_window_dict(window_dict) {
                        windows.push(window);
                    }
                }
            }

            CFRelease(window_list_info);
        }

        debug!("Found {} windows", windows.len());
        Ok(windows)
    }

    unsafe fn parse_window_dict(dict: CFDictionaryRef) -> Option<Window> {
        // Extract window ID
        let window_id = Self::get_number_from_dict(dict, "kCGWindowNumber")? as u32;

        // Extract window title (optional)
        let title = Self::get_string_from_dict(dict, "kCGWindowName")
            .unwrap_or_else(|| "Untitled".to_string());

        // Extract owner name
        let owner = Self::get_string_from_dict(dict, "kCGWindowOwnerName")
            .unwrap_or_else(|| "Unknown".to_string());

        // Extract owner PID
        let owner_pid =
            Self::get_number_from_dict(dict, "kCGWindowOwnerPID").unwrap_or(-1.0) as i32;

        // Extract window bounds
        let bounds_dict = Self::get_dict_from_dict(dict, "kCGWindowBounds")?;
        let rect = Self::parse_bounds_dict(bounds_dict)?;

        // Extract layer (to determine if window should be managed)
        let layer = Self::get_number_from_dict(dict, "kCGWindowLayer").unwrap_or(0.0) as i64;

        // Extract on-screen status
        let is_on_screen =
            Self::get_number_from_dict(dict, "kCGWindowIsOnscreen").unwrap_or(0.0) != 0.0;

        // Extract alpha (transparency)
        let alpha = Self::get_number_from_dict(dict, "kCGWindowAlpha").unwrap_or(1.0);

        // Extract workspace ID (macOS Space)
        let workspace_id =
            Self::get_number_from_dict(dict, "kCGWindowWorkspace").unwrap_or(1.0) as u32;

        // Filter out desktop elements, dock, menu bar, etc.
        // Layer 0 is normal application windows
        if layer != 0 || !is_on_screen || alpha < 0.1 {
            debug!(
                "Filtering out window {} ({}): layer={}, on_screen={}, alpha={}",
                title, owner, layer, is_on_screen, alpha
            );
            return None;
        }

        // Skip very small windows (likely system elements)
        if rect.width < 50.0 || rect.height < 50.0 {
            debug!(
                "Filtering out small window {} ({}): {}x{}",
                title, owner, rect.width, rect.height
            );
            return None;
        }

        // Filter out known system applications that should never be tiled
        let never_tile_apps = [
            "Dock",
            "SystemUIServer",
            "Control Center",
            "NotificationCenter",
            "WindowServer",
            "loginwindow",
            "Spotlight",
            "CoreServicesUIAgent",
            "Menubar",
            "Menu Bar",
            "SystemPreferences",
        ];

        if never_tile_apps.contains(&owner.as_str()) {
            return None;
        }

        // Skip very obvious system panels, but allow most application windows
        if title.is_empty() && (owner.contains("System") || owner.len() < 3) {
            debug!("Filtering out system window {} ({})", title, owner);
            return None;
        }

        debug!(
            "Successfully parsed window: {} ({}) on workspace {}",
            title, owner, workspace_id
        );

        Some(Window {
            id: WindowId(window_id),
            title,
            owner,
            owner_pid,
            rect,
            is_minimized: false, // We'll need to check this separately
            is_focused: false,   // We'll need to check this separately
            workspace_id,        // Now properly detected from macOS
        })
    }

    unsafe fn get_string_from_dict(dict: CFDictionaryRef, key: &str) -> Option<String> {
        let key_cstr = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if cf_key.is_null() {
            return None;
        }

        let value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if value.is_null() {
            return None;
        }

        let cf_string = value as CFStringRef;
        let length = CFStringGetLength(cf_string);

        if length == 0 {
            return Some(String::new());
        }

        let mut buffer = vec![0u8; (length as usize) * 4 + 1]; // UTF-8 can be up to 4 bytes per char

        if CFStringGetCString(
            cf_string,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
        ) {
            // Find the null terminator
            if let Some(null_pos) = buffer.iter().position(|&b| b == 0) {
                buffer.truncate(null_pos);
            }
            String::from_utf8(buffer).ok()
        } else {
            None
        }
    }

    unsafe fn get_number_from_dict(dict: CFDictionaryRef, key: &str) -> Option<f64> {
        let key_cstr = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if cf_key.is_null() {
            return None;
        }

        let value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if value.is_null() {
            return None;
        }

        let cf_number = value as CFNumberRef;
        let mut result: f64 = 0.0;

        if CFNumberGetValue(
            cf_number,
            K_CF_NUMBER_DOUBLE_TYPE,
            &mut result as *mut f64 as *mut c_void,
        ) {
            Some(result)
        } else {
            None
        }
    }

    unsafe fn get_dict_from_dict(dict: CFDictionaryRef, key: &str) -> Option<CFDictionaryRef> {
        let key_cstr = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if cf_key.is_null() {
            return None;
        }

        let value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if value.is_null() {
            None
        } else {
            Some(value as CFDictionaryRef)
        }
    }

    unsafe fn parse_bounds_dict(bounds_dict: CFDictionaryRef) -> Option<Rect> {
        let x = Self::get_number_from_dict(bounds_dict, "X")?;
        let y = Self::get_number_from_dict(bounds_dict, "Y")?;
        let width = Self::get_number_from_dict(bounds_dict, "Width")?;
        let height = Self::get_number_from_dict(bounds_dict, "Height")?;

        Some(Rect::new(x, y, width, height))
    }

    pub fn get_window_info_by_id(window_id: u32) -> Result<Option<Window>> {
        let windows = Self::get_all_windows()?;
        Ok(windows.into_iter().find(|w| w.id.0 == window_id))
    }

    pub fn get_windows_by_owner(owner_name: &str) -> Result<Vec<Window>> {
        let windows = Self::get_all_windows()?;
        Ok(windows
            .into_iter()
            .filter(|w| w.owner == owner_name)
            .collect())
    }

    pub fn get_focused_window_info() -> Result<Option<Window>> {
        // For now, use the window enumeration approach
        // In a full implementation, we'd use AXUIElementCopyAttributeValue with kAXFocusedWindowAttribute
        let windows = Self::get_all_windows()?;

        // Since we can't easily determine focus from CGWindowListCopyWindowInfo alone,
        // we return the first window for now. A complete implementation would need
        // Accessibility API calls to determine the truly focused window.
        Ok(windows.into_iter().next())
    }
}

// Window cache for efficient lookups
pub struct WindowCache {
    windows: HashMap<WindowId, Window>,
    last_update: std::time::Instant,
    cache_duration: std::time::Duration,
}

impl Default for WindowCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowCache {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            last_update: std::time::Instant::now(),
            cache_duration: std::time::Duration::from_millis(100), // Cache for 100ms
        }
    }

    pub fn get_windows(&mut self) -> Result<&HashMap<WindowId, Window>> {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_update) > self.cache_duration {
            self.refresh()?;
        }
        Ok(&self.windows)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let windows = CGWindowInfo::get_all_windows()?;
        self.windows.clear();
        for window in windows {
            self.windows.insert(window.id, window);
        }
        self.last_update = std::time::Instant::now();
        debug!("Window cache refreshed with {} windows", self.windows.len());
        Ok(())
    }

    pub fn get_window(&mut self, id: WindowId) -> Result<Option<&Window>> {
        let windows = self.get_windows()?;
        Ok(windows.get(&id))
    }
}
