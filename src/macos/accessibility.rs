use crate::{Rect, Result, WindowId};
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use log::{debug, info, warn};
use std::collections::HashMap;

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

            // Create a window ID from the memory address (simple approach)
            let window_id = WindowId(focused_window as u32);

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
            debug!("Window {:?} not found in accessibility cache - may be a non-manageable window", window_id);
        }

        Ok(())
    }

    pub fn move_window(&mut self, window_id: WindowId, _rect: Rect) -> Result<()> {
        debug!("Moving window {:?} via Accessibility API", window_id);

        // Try to refresh cache if window not found and cache is stale
        if !self.window_cache.contains_key(&window_id) {
            if let Err(e) = self.refresh_window_cache() {
                warn!("Failed to refresh window cache: {}", e);
            }
        }

        if let Some((_pid, _element)) = self.window_cache.get(&window_id) {
            // Window movement implementation would require complex Core Foundation dictionary creation
            // For now, just log the action
            debug!(
                "Window movement not yet fully implemented - requires proper CF dictionary setup"
            );
        } else {
            debug!("Window {:?} not found in accessibility cache - may be a non-manageable window", window_id);
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
            debug!("Window {:?} not found in accessibility cache - may be a non-manageable window", window_id);
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

        // For now, we'll use a simple approach - clear the cache and let it be rebuilt on demand
        // A more sophisticated approach would enumerate all applications and their windows
        // via the Accessibility API and map them to CGWindow IDs
        self.window_cache.clear();
        self.last_cache_update = now;
        
        debug!("Accessibility window cache refreshed");
        Ok(())
    }
}

impl Drop for AccessibilityManager {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.system_element);
        }
    }
}
