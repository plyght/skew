use crate::{Rect, WindowId};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use log::{debug, info, warn};
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum WindowDragEvent {
    DragStarted {
        window_id: WindowId,
        initial_rect: Rect,
        owner_pid: i32,
    },
    DragEnded {
        window_id: WindowId,
        final_rect: Rect,
        owner_pid: i32,
    },
}

pub struct WindowDragNotificationObserver {
    event_sender: mpsc::Sender<WindowDragEvent>,
    observer: Option<id>,
    dragging_windows: Arc<Mutex<HashMap<WindowId, Rect>>>,
}

impl WindowDragNotificationObserver {
    pub fn new(event_sender: mpsc::Sender<WindowDragEvent>) -> Self {
        Self {
            event_sender,
            observer: None,
            dragging_windows: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_observing(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let notification_center: id = {
                let ns_notification_center_class = Class::get("NSNotificationCenter").unwrap();
                msg_send![ns_notification_center_class, defaultCenter]
            };

            // Create observer for window move notifications
            let observer_class = self.create_observer_class()?;
            let observer: id = msg_send![observer_class, new];

            // Store reference to self in the observer for callbacks
            let dragging_windows = Arc::clone(&self.dragging_windows);
            let event_sender = self.event_sender.clone();

            // Set up the observer with our callback data
            (*observer).set_ivar(
                "dragging_windows",
                Box::into_raw(Box::new(dragging_windows)) as *const _ as *const std::ffi::c_void,
            );
            (*observer).set_ivar(
                "event_sender",
                Box::into_raw(Box::new(event_sender)) as *const _ as *const std::ffi::c_void,
            );

            // Register for NSWindowWillMoveNotification
            let will_move_name = NSString::alloc(nil).init_str("NSWindowWillMoveNotification");
            let will_move_selector = sel!(windowWillMove:);
            let _: () = msg_send![notification_center,
                addObserver: observer
                selector: will_move_selector
                name: will_move_name
                object: nil
            ];

            // Register for NSWindowDidMoveNotification
            let did_move_name = NSString::alloc(nil).init_str("NSWindowDidMoveNotification");
            let did_move_selector = sel!(windowDidMove:);
            let _: () = msg_send![notification_center,
                addObserver: observer
                selector: did_move_selector
                name: did_move_name
                object: nil
            ];

            self.observer = Some(observer);
            info!("Window drag notification observer started successfully");
            Ok(())
        }
    }

    pub fn stop_observing(&mut self) {
        if let Some(observer) = self.observer.take() {
            unsafe {
                let notification_center: id = {
                    let ns_notification_center_class = Class::get("NSNotificationCenter").unwrap();
                    msg_send![ns_notification_center_class, defaultCenter]
                };
                let _: () = msg_send![notification_center, removeObserver: observer];

                // Clean up the observer object
                let _: () = msg_send![observer, release];
            }
        }
    }

    unsafe fn create_observer_class(&self) -> Result<*const Class, Box<dyn std::error::Error>> {
        let superclass = class!(NSObject);
        let mut decl = objc::declare::ClassDecl::new("WindowDragObserver", superclass)
            .ok_or("Failed to create class declaration")?;

        // Add instance variables to store our callback data
        decl.add_ivar::<*const std::ffi::c_void>("dragging_windows");
        decl.add_ivar::<*const std::ffi::c_void>("event_sender");

        // Add windowWillMove: method
        decl.add_method(
            sel!(windowWillMove:),
            window_will_move_callback as extern "C" fn(&mut Object, Sel, id),
        );

        // Add windowDidMove: method
        decl.add_method(
            sel!(windowDidMove:),
            window_did_move_callback as extern "C" fn(&mut Object, Sel, id),
        );

        Ok(decl.register())
    }
}

impl Drop for WindowDragNotificationObserver {
    fn drop(&mut self) {
        self.stop_observing();
    }
}

extern "C" fn window_will_move_callback(observer: &mut Object, _cmd: Sel, notification: id) {
    unsafe {
        debug!("NSWindowWillMoveNotification received");

        let window: id = msg_send![notification, object];
        if window == nil {
            return;
        }

        // Get window ID, initial rect, and owner PID
        if let (Some(window_id), Some(rect), Some(owner_pid)) = (
            get_window_id(window),
            get_window_rect(window),
            get_window_owner_pid(window),
        ) {
            debug!(
                "Window drag started: {:?} at {:?} (PID: {})",
                window_id, rect, owner_pid
            );

            // Get our callback data from the observer
            if let (Some(dragging_windows), Some(event_sender)) =
                (get_dragging_windows(observer), get_event_sender(observer))
            {
                // Store initial position
                dragging_windows.lock().unwrap().insert(window_id, rect);

                // Send drag started event
                let event = WindowDragEvent::DragStarted {
                    window_id,
                    initial_rect: rect,
                    owner_pid,
                };

                if let Err(e) = event_sender.try_send(event) {
                    warn!("Failed to send drag started event: {}", e);
                }
            }
        }
    }
}

extern "C" fn window_did_move_callback(observer: &mut Object, _cmd: Sel, notification: id) {
    unsafe {
        debug!("NSWindowDidMoveNotification received");

        let window: id = msg_send![notification, object];
        if window == nil {
            return;
        }

        // Get window ID, final rect, and owner PID
        if let (Some(window_id), Some(final_rect), Some(owner_pid)) = (
            get_window_id(window),
            get_window_rect(window),
            get_window_owner_pid(window),
        ) {
            debug!(
                "Window drag ended: {:?} at {:?} (PID: {})",
                window_id, final_rect, owner_pid
            );

            // Get our callback data from the observer
            if let (Some(dragging_windows), Some(event_sender)) =
                (get_dragging_windows(observer), get_event_sender(observer))
            {
                // Check if this window was being dragged
                if dragging_windows
                    .lock()
                    .unwrap()
                    .remove(&window_id)
                    .is_some()
                {
                    // Send drag ended event
                    let event = WindowDragEvent::DragEnded {
                        window_id,
                        final_rect,
                        owner_pid,
                    };

                    if let Err(e) = event_sender.try_send(event) {
                        warn!("Failed to send drag ended event: {}", e);
                    }
                }
            }
        }
    }
}

unsafe fn get_window_id(window: id) -> Option<WindowId> {
    // Get window number (NSWindow windowNumber)
    let window_number: i32 = msg_send![window, windowNumber];
    if window_number > 0 {
        Some(WindowId(window_number as u32))
    } else {
        None
    }
}

unsafe fn get_window_rect(window: id) -> Option<Rect> {
    // Get window frame
    let frame: cocoa::foundation::NSRect = msg_send![window, frame];

    // Convert from Cocoa coordinates (origin at bottom-left) to our coordinates (origin at top-left)
    let screen_height = {
        let main_screen: id = {
            let ns_screen_class = Class::get("NSScreen").unwrap();
            msg_send![ns_screen_class, mainScreen]
        };
        let screen_frame: cocoa::foundation::NSRect = msg_send![main_screen, frame];
        screen_frame.size.height
    };

    let y_flipped = screen_height - frame.origin.y - frame.size.height;

    Some(Rect::new(
        frame.origin.x,
        y_flipped,
        frame.size.width,
        frame.size.height,
    ))
}

unsafe fn get_window_owner_pid(window: id) -> Option<i32> {
    let window_number: i32 = msg_send![window, windowNumber];
    if window_number <= 0 {
        return None;
    }

    // Use NSRunningApplication to get the PID from the window's owning app
    let app: id = msg_send![window, app];
    if app != nil {
        let running_app: id = msg_send![app, runningApplication];
        if running_app != nil {
            let pid: i32 = msg_send![running_app, processIdentifier];
            return Some(pid);
        }
    }

    None
}

unsafe fn get_dragging_windows(observer: &Object) -> Option<Arc<Mutex<HashMap<WindowId, Rect>>>> {
    let ptr: *const std::ffi::c_void = *observer.get_ivar("dragging_windows");
    if ptr.is_null() {
        return None;
    }
    let boxed = Box::from_raw(ptr as *mut Arc<Mutex<HashMap<WindowId, Rect>>>);
    let result = Some((*boxed).clone());
    let _ = Box::into_raw(boxed); // Don't drop it
    result
}

unsafe fn get_event_sender(observer: &Object) -> Option<mpsc::Sender<WindowDragEvent>> {
    let ptr: *const std::ffi::c_void = *observer.get_ivar("event_sender");
    if ptr.is_null() {
        return None;
    }
    let boxed = Box::from_raw(ptr as *mut mpsc::Sender<WindowDragEvent>);
    let result = Some((*boxed).clone());
    let _ = Box::into_raw(boxed); // Don't drop it
    result
}
