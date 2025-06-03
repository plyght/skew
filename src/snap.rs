use crate::{Rect, Window, WindowId};
use log::debug;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapRegion {
    Center,
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl SnapRegion {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Center => "Center",
            Self::North => "North",
            Self::South => "South",
            Self::East => "East",
            Self::West => "West",
            Self::NorthEast => "NorthEast",
            Self::NorthWest => "NorthWest",
            Self::SouthEast => "SouthEast",
            Self::SouthWest => "SouthWest",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnapZone {
    pub region: SnapRegion,
    pub bounds: Rect,
    pub snap_rect: Rect,
}

#[derive(Debug, Clone)]
pub enum DragResult {
    SnapToZone(Rect),
    SwapWithWindow(WindowId),
    ReturnToOriginal(Rect),
    NoAction,
}

pub struct SnapManager {
    screen_rect: Rect,
    snap_zones: Vec<SnapZone>,
    #[allow(dead_code)]
    snap_threshold: f64,
    window_drag_states: HashMap<WindowId, WindowDragState>,
}

#[derive(Debug, Clone)]
struct WindowDragState {
    #[allow(dead_code)]
    window_id: WindowId,
    initial_rect: Rect,
    is_dragging: bool,
    drag_start_time: std::time::Instant,
}

impl SnapManager {
    pub fn new(screen_rect: Rect, snap_threshold: f64) -> Self {
        let mut manager = Self {
            screen_rect,
            snap_zones: Vec::new(),
            snap_threshold,
            window_drag_states: HashMap::new(),
        };
        manager.update_snap_zones(screen_rect);
        manager
    }

    pub fn update_screen_rect(&mut self, screen_rect: Rect) {
        self.screen_rect = screen_rect;
        self.update_snap_zones(screen_rect);
    }

    fn update_snap_zones(&mut self, screen_rect: Rect) {
        self.snap_zones.clear();

        let border_size = 120.0; // Size of snap zone borders
        let margin = 10.0; // Margin from screen edges

        debug!("Creating snap zones for screen: {:?}", screen_rect);

        // Center region (much smaller - only the very center of the screen)
        let center_bounds = Rect::new(
            screen_rect.x + screen_rect.width * 0.3,
            screen_rect.y + screen_rect.height * 0.3,
            screen_rect.width * 0.4,
            screen_rect.height * 0.4,
        );

        debug!("Center zone bounds: {:?}", center_bounds);

        // Center snap rect (smaller, leaves room for other windows)
        let center_snap = Rect::new(
            screen_rect.x + screen_rect.width * 0.25,
            screen_rect.y + screen_rect.height * 0.25,
            screen_rect.width * 0.5,
            screen_rect.height * 0.5,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::Center,
            bounds: center_bounds,
            snap_rect: center_snap,
        });

        // North region (top edge)
        let north_bounds = Rect::new(
            screen_rect.x + border_size,
            screen_rect.y,
            screen_rect.width - 2.0 * border_size,
            border_size,
        );

        debug!("North zone bounds: {:?}", north_bounds);

        let north_snap = Rect::new(
            screen_rect.x + margin,
            screen_rect.y + margin,
            screen_rect.width - 2.0 * margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::North,
            bounds: north_bounds,
            snap_rect: north_snap,
        });

        // South region (bottom edge)
        let south_bounds = Rect::new(
            screen_rect.x + border_size,
            screen_rect.y + screen_rect.height - border_size,
            screen_rect.width - 2.0 * border_size,
            border_size,
        );

        debug!("South zone bounds: {:?}", south_bounds);

        let south_snap = Rect::new(
            screen_rect.x + margin,
            screen_rect.y + screen_rect.height * 0.5,
            screen_rect.width - 2.0 * margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::South,
            bounds: south_bounds,
            snap_rect: south_snap,
        });

        // West region (left edge)
        let west_bounds = Rect::new(
            screen_rect.x,
            screen_rect.y + border_size,
            border_size,
            screen_rect.height - 2.0 * border_size,
        );

        debug!("West zone bounds: {:?}", west_bounds);

        let west_snap = Rect::new(
            screen_rect.x + margin,
            screen_rect.y + margin,
            screen_rect.width * 0.5 - margin,
            screen_rect.height - 2.0 * margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::West,
            bounds: west_bounds,
            snap_rect: west_snap,
        });

        // East region (right edge)
        let east_bounds = Rect::new(
            screen_rect.x + screen_rect.width - border_size,
            screen_rect.y + border_size,
            border_size,
            screen_rect.height - 2.0 * border_size,
        );

        debug!("East zone bounds: {:?}", east_bounds);

        let east_snap = Rect::new(
            screen_rect.x + screen_rect.width * 0.5,
            screen_rect.y + margin,
            screen_rect.width * 0.5 - margin,
            screen_rect.height - 2.0 * margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::East,
            bounds: east_bounds,
            snap_rect: east_snap,
        });

        // Corner regions for more precise placement

        // Northwest corner
        let nw_bounds = Rect::new(screen_rect.x, screen_rect.y, border_size, border_size);
        let nw_snap = Rect::new(
            screen_rect.x + margin,
            screen_rect.y + margin,
            screen_rect.width * 0.5 - margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::NorthWest,
            bounds: nw_bounds,
            snap_rect: nw_snap,
        });

        // Northeast corner
        let ne_bounds = Rect::new(
            screen_rect.x + screen_rect.width - border_size,
            screen_rect.y,
            border_size,
            border_size,
        );
        let ne_snap = Rect::new(
            screen_rect.x + screen_rect.width * 0.5,
            screen_rect.y + margin,
            screen_rect.width * 0.5 - margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::NorthEast,
            bounds: ne_bounds,
            snap_rect: ne_snap,
        });

        // Southwest corner
        let sw_bounds = Rect::new(
            screen_rect.x,
            screen_rect.y + screen_rect.height - border_size,
            border_size,
            border_size,
        );
        let sw_snap = Rect::new(
            screen_rect.x + margin,
            screen_rect.y + screen_rect.height * 0.5,
            screen_rect.width * 0.5 - margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::SouthWest,
            bounds: sw_bounds,
            snap_rect: sw_snap,
        });

        // Southeast corner
        let se_bounds = Rect::new(
            screen_rect.x + screen_rect.width - border_size,
            screen_rect.y + screen_rect.height - border_size,
            border_size,
            border_size,
        );
        let se_snap = Rect::new(
            screen_rect.x + screen_rect.width * 0.5,
            screen_rect.y + screen_rect.height * 0.5,
            screen_rect.width * 0.5 - margin,
            screen_rect.height * 0.5 - margin,
        );

        self.snap_zones.push(SnapZone {
            region: SnapRegion::SouthEast,
            bounds: se_bounds,
            snap_rect: se_snap,
        });
    }

    pub fn start_window_drag(&mut self, window_id: WindowId, current_rect: Rect) {
        self.window_drag_states.insert(
            window_id,
            WindowDragState {
                window_id,
                initial_rect: current_rect,
                is_dragging: true,
                drag_start_time: std::time::Instant::now(),
            },
        );
    }

    pub fn update_window_drag(&mut self, window_id: WindowId, _current_rect: Rect) {
        if let Some(drag_state) = self.window_drag_states.get_mut(&window_id) {
            drag_state.is_dragging = true;
        }
    }

    pub fn end_window_drag(
        &mut self,
        window_id: WindowId,
        final_rect: Rect,
        all_windows: &[&Window],
    ) -> DragResult {
        debug!(
            "🎬 SnapManager::end_window_drag called for window {:?}",
            window_id
        );

        if let Some(drag_state) = self.window_drag_states.remove(&window_id) {
            // Check if the window was dragged for a meaningful amount of time/distance
            let drag_duration = drag_state.drag_start_time.elapsed();
            let drag_distance = self.calculate_drag_distance(&drag_state.initial_rect, &final_rect);

            debug!(
                "🕐 Drag ended for window {:?}: duration={}ms, distance={:.1}px, initial={:?}, final={:?}",
                window_id,
                drag_duration.as_millis(),
                drag_distance,
                drag_state.initial_rect,
                final_rect
            );

            // More responsive thresholds for better user experience
            if drag_duration.as_millis() > 100 && drag_distance > 20.0 {
                debug!("✅ Drag qualifies for processing, checking targets...");

                // First check if we're over another window in the center area for swapping
                if let Some(target_window_id) =
                    self.find_window_under_drag(window_id, final_rect, all_windows)
                {
                    debug!(
                        "🔍 Found window {:?} under dragged window",
                        target_window_id
                    );
                    let center_x = final_rect.x + final_rect.width / 2.0;
                    let center_y = final_rect.y + final_rect.height / 2.0;

                    // Check if we're in the center swap zone (not in edge zones)
                    let in_edge_zone = self.snap_zones.iter().any(|zone| {
                        matches!(
                            zone.region,
                            SnapRegion::North
                                | SnapRegion::South
                                | SnapRegion::East
                                | SnapRegion::West
                        ) && self.point_in_rect(center_x, center_y, &zone.bounds)
                    });

                    if !in_edge_zone {
                        debug!(
                            "🔄 Window dropped over another window in swap zone, initiating swap"
                        );
                        return DragResult::SwapWithWindow(target_window_id);
                    } else {
                        debug!("📍 Window is in edge zone, not swapping");
                    }
                } else {
                    debug!("🚫 No window found under dragged window");
                }

                // Check for snap zone targets
                if let Some(snap_rect) = self.find_snap_target(final_rect) {
                    debug!("🎯 Found snap target: {:?}", snap_rect);
                    // Double-check that we're not snapping to the same position
                    let dx = (snap_rect.x - final_rect.x).abs();
                    let dy = (snap_rect.y - final_rect.y).abs();
                    if dx > 10.0 || dy > 10.0 {
                        debug!("📌 Snapping to zone");
                        return DragResult::SnapToZone(snap_rect);
                    } else {
                        debug!(
                            "↩️ Snap target too close to current position, returning to original"
                        );
                        return DragResult::ReturnToOriginal(drag_state.initial_rect);
                    }
                } else {
                    debug!("🎯❌ No snap zone found");
                }

                // If no valid target found, return to original position
                debug!("↩️ No valid snap target found, returning to original position");
                return DragResult::ReturnToOriginal(drag_state.initial_rect);
            } else {
                debug!(
                    "⏱️ Drag too short/small for processing (duration: {}ms < 100ms OR distance: {:.1}px < 20px), returning to original",
                    drag_duration.as_millis(),
                    drag_distance
                );
                return DragResult::ReturnToOriginal(drag_state.initial_rect);
            }
        } else {
            debug!("❌ No drag state found for window {:?}", window_id);
        }
        DragResult::NoAction
    }

    fn calculate_drag_distance(&self, initial: &Rect, final_rect: &Rect) -> f64 {
        let dx = final_rect.x - initial.x;
        let dy = final_rect.y - initial.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn find_snap_target(&self, window_rect: Rect) -> Option<Rect> {
        // Use the window's center point to determine which snap zone it's in
        let center_x = window_rect.x + window_rect.width / 2.0;
        let center_y = window_rect.y + window_rect.height / 2.0;

        // Check which zone the window center is in and return the first match
        // The order matters: corners, then sides, then center

        // Check corners first (they're more specific)
        for zone in &self.snap_zones {
            if matches!(
                zone.region,
                SnapRegion::NorthWest
                    | SnapRegion::NorthEast
                    | SnapRegion::SouthWest
                    | SnapRegion::SouthEast
            ) && self.point_in_rect(center_x, center_y, &zone.bounds)
            {
                debug!(
                    "Window center ({}, {}) in {} zone",
                    center_x,
                    center_y,
                    zone.region.name()
                );
                return Some(zone.snap_rect);
            }
        }

        // Then check sides
        for zone in &self.snap_zones {
            if matches!(
                zone.region,
                SnapRegion::North | SnapRegion::South | SnapRegion::East | SnapRegion::West
            ) && self.point_in_rect(center_x, center_y, &zone.bounds)
            {
                debug!(
                    "Window center ({}, {}) in {} zone",
                    center_x,
                    center_y,
                    zone.region.name()
                );
                return Some(zone.snap_rect);
            }
        }

        // Finally check center
        for zone in &self.snap_zones {
            if zone.region == SnapRegion::Center
                && self.point_in_rect(center_x, center_y, &zone.bounds)
            {
                debug!(
                    "Window center ({}, {}) in {} zone",
                    center_x,
                    center_y,
                    zone.region.name()
                );
                return Some(zone.snap_rect);
            }
        }

        debug!(
            "Window center ({}, {}) not in any snap zone",
            center_x, center_y
        );
        None
    }

    fn point_in_rect(&self, x: f64, y: f64, rect: &Rect) -> bool {
        x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
    }

    pub fn find_window_under_drag(
        &self,
        dragged_window_id: WindowId,
        dragged_rect: Rect,
        all_windows: &[&Window],
    ) -> Option<WindowId> {
        let center_x = dragged_rect.x + dragged_rect.width / 2.0;
        let center_y = dragged_rect.y + dragged_rect.height / 2.0;

        for window in all_windows {
            if window.id == dragged_window_id {
                continue;
            }

            if self.point_in_rect(center_x, center_y, &window.rect) {
                debug!(
                    "Found window {:?} under dragged window {:?}",
                    window.id, dragged_window_id
                );
                return Some(window.id);
            }
        }

        None
    }

    pub fn get_snap_zones(&self) -> &[SnapZone] {
        &self.snap_zones
    }

    pub fn is_window_dragging(&self, window_id: WindowId) -> bool {
        self.window_drag_states
            .get(&window_id)
            .map(|state| state.is_dragging)
            .unwrap_or(false)
    }

    pub fn clear_drag_state(&mut self, window_id: WindowId) {
        self.window_drag_states.remove(&window_id);
    }
}
