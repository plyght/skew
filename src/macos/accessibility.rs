use crate::{Rect, Result, WindowId};
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::os::raw::{c_double, c_int, c_void};

// Additional system calls for process enumeration
extern "C" {
    fn proc_listpids(proc_type: u32, typeinfo: u32, buffer: *mut c_int, buffersize: c_int)
        -> c_int;
}

const PROC_ALL_PIDS: u32 = 1;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyElementAtPosition(
        element: AXUIElementRef,
        x: f32,
        y: f32,
        element_at_position: *mut AXUIElementRef,
    ) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;

    // Core Foundation value creation functions
    fn AXValueCreate(value_type: AXValueType, value_ptr: *const c_void) -> CFTypeRef;
}

type AXValueType = u32;
const kAXValueCGPointType: AXValueType = 1;
const kAXValueCGSizeType: AXValueType = 2;

#[repr(C)]
struct CGPoint {
    x: c_double,
    y: c_double,
}

#[repr(C)]
struct CGSize {
    width: c_double,
    height: c_double,
}

type AXUIElementRef = CFTypeRef;
type AXError = i32;

const kAXErrorSuccess: AXError = 0;
const kAXFocusedApplicationAttribute: &str = "AXFocusedApplication";
const kAXFocusedWindowAttribute: &str = "AXFocusedWindow";
const kAXPositionAttribute: &str = "AXPosition";
const kAXSizeAttribute: &str = "AXSize";
const kAXWindowsAttribute: &str = "AXWindows";
const kAXTitleAttribute: &str = "AXTitle";
const kAXRaiseAction: &str = "AXRaise";
const kAXPressAction: &str = "AXPress";

pub struct AccessibilityManager {
    system_element: AXUIElementRef,
    window_cache: HashMap<WindowId, (i32, AXUIElementRef)>, // WindowId -> (pid, element)
    last_cache_update: std::time::Instant,
}

impl AccessibilityManager {
    pub fn new() -> Result<Self> {
        debug!("Initializing Accessibility Manager with real macOS APIs");

        // Check if accessibility permissions are granted
        if unsafe { !AXIsProcessTrusted() } {
            warn!("Accessibility permissions not granted!");
            warn!("Please grant accessibility permissions in System Preferences > Security & Privacy > Privacy > Accessibility");
            warn!("Add this application to the list and enable it.");
        } else {
            info!("Accessibility permissions granted - full functionality available");
        }

        let system_element = unsafe { AXUIElementCreateSystemWide() };

        Ok(Self {
            system_element,
            window_cache: HashMap::new(),
            last_cache_update: std::time::Instant::now(),
        })
    }

    pub fn get_focused_window(&self) -> Result<Option<WindowId>> {
        debug!("Getting focused window via Accessibility API");

        unsafe {
            let focused_app_attr = CFString::new(kAXFocusedApplicationAttribute);
            let mut focused_app: CFTypeRef = std::ptr::null_mut();

            let result = AXUIElementCopyAttributeValue(
                self.system_element,
                focused_app_attr.as_concrete_TypeRef(),
                &mut focused_app,
            );

            if result != kAXErrorSuccess {
                debug!("Failed to get focused application: {}", result);
                return Ok(None);
            }

            let focused_window_attr = CFString::new(kAXFocusedWindowAttribute);
            let mut focused_window: CFTypeRef = std::ptr::null_mut();

            let result = AXUIElementCopyAttributeValue(
                focused_app,
                focused_window_attr.as_concrete_TypeRef(),
                &mut focused_window,
            );

            CFRelease(focused_app);

            if result != kAXErrorSuccess {
                debug!("Failed to get focused window: {}", result);
                return Ok(None);
            }

            // Get window PID to create a unique window ID
            let mut pid: i32 = 0;
            AXUIElementGetPid(focused_window, &mut pid);

            // Create a more stable window ID using hash of element pointer and PID
            // This is still imperfect but better than raw pointer casting
            let ptr_hash = (focused_window as usize).wrapping_mul(2654435761) >> 16;
            let window_id = WindowId(((pid as u64) << 16 | (ptr_hash as u64 & 0xFFFF)) as u32);

            CFRelease(focused_window);

            Ok(Some(window_id))
        }
    }

    pub fn focus_window(&mut self, window_id: WindowId) -> Result<()> {
        debug!("Focusing window {:?} via Accessibility API", window_id);

        // Try to refresh cache if window not found and cache is stale
        if !self.window_cache.contains_key(&window_id) {
            if let Err(e) = self.refresh_window_cache() {
                warn!("Failed to refresh window cache: {}", e);
            }
        }

        if let Some((_pid, element)) = self.window_cache.get(&window_id) {
            unsafe {
                let raise_action = CFString::new(kAXRaiseAction);
                let result = AXUIElementPerformAction(*element, raise_action.as_concrete_TypeRef());

                if result == kAXErrorSuccess {
                    debug!("Successfully focused window {:?}", window_id);
                } else {
                    warn!("Failed to focus window {:?}: error {}", window_id, result);
                }
            }
        } else {
            debug!(
                "Window {:?} not found in accessibility cache - may be a non-manageable window",
                window_id
            );
        }

        Ok(())
    }

    pub fn move_window(&mut self, window_id: WindowId, rect: Rect) -> Result<()> {
        debug!(
            "Moving window {:?} to {:?} via Accessibility API",
            window_id, rect
        );

        // Try direct approach by finding the window element
        if let Some(element) = self.find_window_element(window_id)? {
            let result = self.set_window_rect(element, rect);
            unsafe {
                CFRelease(element); // Release the retained element
            }
            result?;
            debug!("Successfully moved window {:?} to {:?}", window_id, rect);
        } else {
            warn!(
                "Could not find accessibility element for window {:?}",
                window_id
            );
        }

        Ok(())
    }

    fn find_window_element(&mut self, window_id: WindowId) -> Result<Option<AXUIElementRef>> {
        // First check the cache
        if let Some((_, element)) = self.window_cache.get(&window_id) {
            unsafe {
                // Retain the element before returning it
                CFRetain(*element);
                return Ok(Some(*element));
            }
        }

        // If not in cache, try to refresh and look again
        self.refresh_window_cache()?;

        if let Some((_, element)) = self.window_cache.get(&window_id) {
            unsafe {
                // Retain the element before returning it
                CFRetain(*element);
                return Ok(Some(*element));
            }
        }

        debug!("Window {:?} not found in accessibility cache", window_id);
        Ok(None)
    }

    pub fn move_all_windows(
        &mut self,
        layouts: &std::collections::HashMap<crate::WindowId, crate::Rect>,
        windows: &[crate::Window],
    ) -> Result<()> {
        debug!("Moving ALL desktop windows using accessibility API");

        // Debug: show the layouts we're supposed to apply
        for (window_id, rect) in layouts {
            debug!("Layout for window {:?}: {:?}", window_id, rect);
        }

        // Get unique PIDs from the windows we need to tile
        let mut unique_pids = std::collections::HashSet::new();
        for window in windows {
            unique_pids.insert(window.owner_pid);
        }

        debug!("Getting windows for PIDs: {:?}", unique_pids);

        let mut all_window_elements = Vec::new();
        for pid in unique_pids {
            match self.get_windows_for_app_by_pid(pid) {
                Ok(mut app_windows) => {
                    debug!(
                        "Successfully got {} window elements for PID {}",
                        app_windows.len(),
                        pid
                    );
                    all_window_elements.append(&mut app_windows);
                }
                Err(e) => {
                    warn!("Failed to get windows for PID {}: {}", pid, e);
                }
            }
        }

        // Create a mapping from window elements to their window IDs
        // This is a best-effort approach since we don't have perfect correlation
        let mut element_to_window: HashMap<AXUIElementRef, WindowId> = HashMap::new();
        let mut element_index = 0;

        for window in windows {
            if element_index < all_window_elements.len() {
                element_to_window.insert(all_window_elements[element_index], window.id);
                element_index += 1;
            }
        }

        debug!(
            "Applying {} layouts to {} accessible window elements",
            layouts.len(),
            all_window_elements.len()
        );

        // Apply layouts by looking up the correct window ID
        for (window_element, window_id) in element_to_window {
            if let Some(rect) = layouts.get(&window_id) {
                debug!("Moving window {:?} element to {:?}", window_id, rect);
                self.set_window_rect(window_element, *rect)?;
            }
        }

        // Clean up retained window elements
        unsafe {
            for window_element in all_window_elements {
                CFRelease(window_element);
            }
        }

        Ok(())
    }

    fn set_window_rect(&self, element: AXUIElementRef, rect: Rect) -> Result<()> {
        unsafe {
            // Create position value using AXValue
            let position = CGPoint {
                x: rect.x,
                y: rect.y,
            };
            let position_value = AXValueCreate(
                kAXValueCGPointType,
                &position as *const CGPoint as *const c_void,
            );

            if position_value.is_null() {
                return Err(anyhow::anyhow!("Failed to create position AXValue"));
            }

            let position_attr = CFString::new(kAXPositionAttribute);
            let position_result = AXUIElementSetAttributeValue(
                element,
                position_attr.as_concrete_TypeRef(),
                position_value,
            );

            // Create size value using AXValue
            let size = CGSize {
                width: rect.width,
                height: rect.height,
            };
            let size_value =
                AXValueCreate(kAXValueCGSizeType, &size as *const CGSize as *const c_void);

            if size_value.is_null() {
                CFRelease(position_value);
                return Err(anyhow::anyhow!("Failed to create size AXValue"));
            }

            let size_attr = CFString::new(kAXSizeAttribute);
            let size_result =
                AXUIElementSetAttributeValue(element, size_attr.as_concrete_TypeRef(), size_value);

            // Clean up
            CFRelease(position_value);
            CFRelease(size_value);

            if position_result == kAXErrorSuccess && size_result == kAXErrorSuccess {
                debug!("Successfully set window rect to {:?}", rect);
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Failed to set window rect: position_error={}, size_error={}",
                    position_result,
                    size_result
                ))
            }
        }
    }

    fn get_all_accessible_window_elements(&mut self) -> Result<Vec<AXUIElementRef>> {
        let mut all_windows = Vec::new();

        // Get windows from ALL accessible applications, not just the focused one
        self.enumerate_all_accessible_applications(&mut all_windows)?;

        debug!(
            "Found {} accessible window elements across all applications",
            all_windows.len()
        );
        Ok(all_windows)
    }

    fn enumerate_all_accessible_applications(
        &mut self,
        window_elements: &mut Vec<AXUIElementRef>,
    ) -> Result<()> {
        // Get ALL running processes and try to get windows from each
        let all_pids = self.get_all_running_pids()?;

        debug!("Found {} total running processes", all_pids.len());

        for pid in all_pids {
            // Try to get windows from this PID
            match self.get_windows_for_app_by_pid(pid) {
                Ok(app_windows) => {
                    if !app_windows.is_empty() {
                        debug!("Found {} windows for PID {}", app_windows.len(), pid);
                        window_elements.extend(app_windows);
                    }
                }
                Err(e) => {
                    // Don't log errors for every PID as many won't have windows
                    debug!("No accessible windows for PID {}: {}", pid, e);
                }
            }
        }

        debug!(
            "Total accessible window elements found: {}",
            window_elements.len()
        );
        Ok(())
    }

    fn get_all_running_pids(&self) -> Result<Vec<i32>> {
        unsafe {
            // First, get the number of PIDs
            let mut buffer = vec![0i32; 1024]; // Start with buffer for 1024 PIDs

            loop {
                let bytes_returned = proc_listpids(
                    PROC_ALL_PIDS,
                    0,
                    buffer.as_mut_ptr(),
                    (buffer.len() * std::mem::size_of::<i32>()) as c_int,
                );

                if bytes_returned < 0 {
                    return Err(anyhow::anyhow!("Failed to list processes"));
                }

                let pids_returned = bytes_returned as usize / std::mem::size_of::<i32>();

                if pids_returned < buffer.len() {
                    // Buffer was large enough, truncate and return
                    buffer.truncate(pids_returned);
                    break;
                } else {
                    // Buffer too small, double it and try again
                    buffer.resize(buffer.len() * 2, 0);
                }
            }

            // Filter out invalid PIDs (0 and negative)
            let valid_pids: Vec<i32> = buffer.into_iter().filter(|&pid| pid > 0).collect();

            Ok(valid_pids)
        }
    }

    #[deprecated(note = "Use enumerate_all_accessible_applications instead")]
    fn try_get_windows_from_other_apps(
        &mut self,
        _window_elements: &mut Vec<AXUIElementRef>,
    ) -> Result<()> {
        debug!("try_get_windows_from_other_apps is deprecated - use enumerate_all_accessible_applications");
        Ok(())
    }

    #[deprecated(note = "Use get_windows_for_app_by_pid instead")]
    fn get_windows_for_app_by_name(&mut self, app_name: &str) -> Result<Vec<AXUIElementRef>> {
        debug!(
            "get_windows_for_app_by_name called for {} - use get_windows_for_app_by_pid instead",
            app_name
        );
        Ok(Vec::new())
    }

    fn get_windows_for_app_by_pid(&mut self, pid: i32) -> Result<Vec<AXUIElementRef>> {
        let mut window_elements = Vec::new();

        // Skip some obvious system processes that can't have windows
        if pid <= 1 {
            return Ok(window_elements);
        }

        unsafe {
            // Create accessibility element for the application
            let app_element = AXUIElementCreateApplication(pid);
            if app_element.is_null() {
                return Ok(window_elements);
            }

            // Get windows for this application
            let windows_attr = CFString::new(kAXWindowsAttribute);
            let mut windows: CFTypeRef = std::ptr::null_mut();

            let windows_result = AXUIElementCopyAttributeValue(
                app_element,
                windows_attr.as_concrete_TypeRef(),
                &mut windows,
            );

            if windows_result == kAXErrorSuccess && !windows.is_null() {
                let array_ref = windows as core_foundation::array::CFArrayRef;
                let count = core_foundation::array::CFArrayGetCount(array_ref);

                if count > 0 {
                    debug!("Processing {} windows for PID {}", count, pid);

                    for i in 0..count {
                        let window_element =
                            core_foundation::array::CFArrayGetValueAtIndex(array_ref, i);
                        if !window_element.is_null() {
                            // Verify this is a valid, manageable window
                            if self.is_window_tileable(window_element) {
                                // Retain the window element so it doesn't get freed when we release the array
                                CFRetain(window_element);
                                window_elements.push(window_element);
                            }
                        }
                    }

                    if !window_elements.is_empty() {
                        debug!(
                            "Found {} tileable windows for PID {}",
                            window_elements.len(),
                            pid
                        );
                    }
                }

                CFRelease(windows);
            }

            CFRelease(app_element);
        }

        Ok(window_elements)
    }

    fn is_window_tileable(&self, window_element: AXUIElementRef) -> bool {
        unsafe {
            // Check if window has position and size attributes (required for tiling)
            let position_attr = CFString::new(kAXPositionAttribute);
            let size_attr = CFString::new(kAXSizeAttribute);

            let mut position_value: CFTypeRef = std::ptr::null_mut();
            let mut size_value: CFTypeRef = std::ptr::null_mut();

            let has_position = AXUIElementCopyAttributeValue(
                window_element,
                position_attr.as_concrete_TypeRef(),
                &mut position_value,
            ) == kAXErrorSuccess;

            let has_size = AXUIElementCopyAttributeValue(
                window_element,
                size_attr.as_concrete_TypeRef(),
                &mut size_value,
            ) == kAXErrorSuccess;

            if !position_value.is_null() {
                CFRelease(position_value);
            }
            if !size_value.is_null() {
                CFRelease(size_value);
            }

            has_position && has_size
        }
    }

    fn get_windows_from_focused_app(
        &mut self,
        window_elements: &mut Vec<AXUIElementRef>,
    ) -> Result<()> {
        unsafe {
            let focused_app_attr = CFString::new(kAXFocusedApplicationAttribute);
            let mut focused_app: CFTypeRef = std::ptr::null_mut();

            let result = AXUIElementCopyAttributeValue(
                self.system_element,
                focused_app_attr.as_concrete_TypeRef(),
                &mut focused_app,
            );

            if result == kAXErrorSuccess && !focused_app.is_null() {
                let windows_attr = CFString::new(kAXWindowsAttribute);
                let mut windows: CFTypeRef = std::ptr::null_mut();

                let windows_result = AXUIElementCopyAttributeValue(
                    focused_app,
                    windows_attr.as_concrete_TypeRef(),
                    &mut windows,
                );

                if windows_result == kAXErrorSuccess && !windows.is_null() {
                    let array_ref = windows as core_foundation::array::CFArrayRef;
                    let count = core_foundation::array::CFArrayGetCount(array_ref);

                    for i in 0..count {
                        let window_element =
                            core_foundation::array::CFArrayGetValueAtIndex(array_ref, i);
                        if !window_element.is_null() {
                            window_elements.push(window_element);
                        }
                    }

                    CFRelease(windows);
                }

                CFRelease(focused_app);
            }
        }

        Ok(())
    }

    pub fn close_window(&mut self, window_id: WindowId) -> Result<()> {
        debug!("Closing window {:?} via Accessibility API", window_id);

        // Try to refresh cache if window not found and cache is stale
        if !self.window_cache.contains_key(&window_id) {
            if let Err(e) = self.refresh_window_cache() {
                warn!("Failed to refresh window cache: {}", e);
            }
        }

        if let Some((_, element)) = self.window_cache.get(&window_id) {
            unsafe {
                // Look for close button
                let windows_attr = CFString::new("AXCloseButton");
                let mut close_button: CFTypeRef = std::ptr::null_mut();

                let result = AXUIElementCopyAttributeValue(
                    *element,
                    windows_attr.as_concrete_TypeRef(),
                    &mut close_button,
                );

                if result == kAXErrorSuccess && !close_button.is_null() {
                    let press_action = CFString::new(kAXPressAction);
                    let press_result =
                        AXUIElementPerformAction(close_button, press_action.as_concrete_TypeRef());

                    if press_result == kAXErrorSuccess {
                        debug!("Successfully closed window {:?}", window_id);
                    } else {
                        warn!("Failed to press close button: error {}", press_result);
                    }

                    CFRelease(close_button);
                } else {
                    warn!("Failed to find close button for window {:?}", window_id);
                }
            }
        } else {
            debug!(
                "Window {:?} not found in accessibility cache - may be a non-manageable window",
                window_id
            );
        }

        Ok(())
    }

    pub fn refresh_window_cache(&mut self) -> Result<()> {
        debug!("Refreshing accessibility window cache");

        let now = std::time::Instant::now();
        // Only refresh if it's been more than 100ms since last refresh
        if now.duration_since(self.last_cache_update) < std::time::Duration::from_millis(100) {
            return Ok(());
        }

        // Release all cached elements before clearing
        unsafe {
            for (_, element) in self.window_cache.values() {
                CFRelease(*element);
            }
        }
        self.window_cache.clear();

        // Get all windows from focused application (limited implementation)
        self.enumerate_focused_app_windows()?;

        self.last_cache_update = now;
        debug!(
            "Accessibility window cache refreshed with {} windows",
            self.window_cache.len()
        );
        Ok(())
    }

    fn enumerate_focused_app_windows(&mut self) -> Result<()> {
        unsafe {
            // Get windows from the focused application only
            // This is a limited implementation - a full implementation would enumerate all apps
            // via NSWorkspace.runningApplications or similar APIs

            // Get focused application windows
            let focused_app_attr = CFString::new(kAXFocusedApplicationAttribute);
            let mut focused_app: CFTypeRef = std::ptr::null_mut();

            let result = AXUIElementCopyAttributeValue(
                self.system_element,
                focused_app_attr.as_concrete_TypeRef(),
                &mut focused_app,
            );

            if result == kAXErrorSuccess && !focused_app.is_null() {
                let mut pid: i32 = 0;
                AXUIElementGetPid(focused_app, &mut pid);

                // Get all windows for this application
                let windows_attr = CFString::new(kAXWindowsAttribute);
                let mut windows: CFTypeRef = std::ptr::null_mut();

                let windows_result = AXUIElementCopyAttributeValue(
                    focused_app,
                    windows_attr.as_concrete_TypeRef(),
                    &mut windows,
                );

                if windows_result == kAXErrorSuccess && !windows.is_null() {
                    self.process_windows_array(windows, pid)?;
                    CFRelease(windows);
                }

                CFRelease(focused_app);
            }
        }

        Ok(())
    }

    fn process_windows_array(&mut self, windows_array: CFTypeRef, pid: i32) -> Result<()> {
        unsafe {
            let array_ref = windows_array as core_foundation::array::CFArrayRef;
            let count = core_foundation::array::CFArrayGetCount(array_ref);

            for i in 0..count {
                let window_element = core_foundation::array::CFArrayGetValueAtIndex(array_ref, i);
                if !window_element.is_null() {
                    // Retain the element before caching it
                    CFRetain(window_element);

                    // Create a more stable window ID by combining PID and window index
                    // This reduces collisions compared to raw pointer casting
                    let base_id = (pid as u64) << 16 | (i as u64);
                    let window_id = WindowId(base_id as u32);
                    self.window_cache.insert(window_id, (pid, window_element));
                }
            }
        }

        Ok(())
    }
}

impl Drop for AccessibilityManager {
    fn drop(&mut self) {
        unsafe {
            // Release all cached window elements
            for (_, element) in self.window_cache.values() {
                CFRelease(*element);
            }
            CFRelease(self.system_element);
        }
    }
}
