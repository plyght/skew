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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoordinateValue {
    Absolute(f64),
    Relative(f64),
}

impl CoordinateValue {
    pub fn resolve(&self, base_value: f64) -> f64 {
        match self {
            CoordinateValue::Absolute(value) => *value,
            CoordinateValue::Relative(percentage) => base_value * percentage,
        }
    }
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
    SwapWithWindow(WindowId, Rect), // WindowId to swap with, original rect of dragged window
    ReturnToOriginal(Rect),
    NoAction,
}

pub struct SnapManager {
    screen_rect: Rect,
    snap_zones: Vec<SnapZone>,
    snap_threshold: f64,
    edge_zone_width: f64,
    corner_size: f64,
    margin: f64,
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
    pub fn new(screen_rect: Rect, snap_threshold: f64, edge_zone_width: f64, corner_size: f64, margin: f64) -> Self {
        let mut manager = Self {
            screen_rect,
            snap_zones: Vec::new(),
            snap_threshold,
            edge_zone_width,
            corner_size,
            margin,
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

        let edge_zone_width = self.edge_zone_width;
        let corner_size = self.corner_size;
        let margin = self.margin;

        debug!("Creating snap zones for screen: {:?}", screen_rect);

        // Define zone configurations based on visual representation
        let zones = [
            // Center swap zone - larger area for swapping windows
            (
                SnapRegion::Center,
                (0.2, 0.2, 0.6, 0.6), // Zone bounds: center 60% of screen for easier targeting
                (0.25, 0.25, 0.5, 0.5), // Snap rect: center quarter if no swap target
            ),
            // Warp zones - edge zones that "warp" windows to screen sides
            (
                SnapRegion::North,
                (
                    corner_size,
                    0.0,
                    screen_rect.width - 2.0 * corner_size,
                    edge_zone_width,
                ), // Top edge zone
                (
                    margin,
                    margin,
                    screen_rect.width - 2.0 * margin,
                    screen_rect.height * 0.5 - margin,
                ), // Snap to top half
            ),
            (
                SnapRegion::South,
                (
                    corner_size,
                    screen_rect.height - edge_zone_width,
                    screen_rect.width - 2.0 * corner_size,
                    edge_zone_width,
                ), // Bottom edge zone
                (
                    margin,
                    screen_rect.height * 0.5,
                    screen_rect.width - 2.0 * margin,
                    screen_rect.height * 0.5 - margin,
                ), // Snap to bottom half
            ),
            (
                SnapRegion::West,
                (
                    0.0,
                    corner_size,
                    edge_zone_width,
                    screen_rect.height - 2.0 * corner_size,
                ), // Left edge zone
                (
                    margin,
                    margin,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height - 2.0 * margin,
                ), // Snap to left half
            ),
            (
                SnapRegion::East,
                (
                    screen_rect.width - edge_zone_width,
                    corner_size,
                    edge_zone_width,
                    screen_rect.height - 2.0 * corner_size,
                ), // Right edge zone
                (
                    screen_rect.width * 0.5,
                    margin,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height - 2.0 * margin,
                ), // Snap to right half
            ),
            // Corner zones for quarter-screen snapping
            (
                SnapRegion::NorthWest,
                (0.0, 0.0, corner_size, corner_size),
                (
                    margin,
                    margin,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height * 0.5 - margin,
                ),
            ),
            (
                SnapRegion::NorthEast,
                (
                    screen_rect.width - corner_size,
                    0.0,
                    corner_size,
                    corner_size,
                ),
                (
                    screen_rect.width * 0.5,
                    margin,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height * 0.5 - margin,
                ),
            ),
            (
                SnapRegion::SouthWest,
                (
                    0.0,
                    screen_rect.height - corner_size,
                    corner_size,
                    corner_size,
                ),
                (
                    margin,
                    screen_rect.height * 0.5,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height * 0.5 - margin,
                ),
            ),
            (
                SnapRegion::SouthEast,
                (
                    screen_rect.width - corner_size,
                    screen_rect.height - corner_size,
                    corner_size,
                    corner_size,
                ),
                (
                    screen_rect.width * 0.5,
                    screen_rect.height * 0.5,
                    screen_rect.width * 0.5 - margin,
                    screen_rect.height * 0.5 - margin,
                ),
            ),
        ];

        for (region, bounds_config, snap_config) in zones {
            let bounds = self.create_absolute_zone_rect(screen_rect, bounds_config);
            let snap_rect = self.create_absolute_zone_rect(screen_rect, snap_config);

            debug!("{} zone bounds: {:?}", region.name(), bounds);

            self.snap_zones.push(SnapZone {
                region,
                bounds,
                snap_rect,
            });
        }
    }

    fn create_absolute_zone_rect(&self, screen_rect: Rect, config: (f64, f64, f64, f64)) -> Rect {
        let (x_config, y_config, w_config, h_config) = config;
        
        // Convert legacy tuple format to explicit coordinate values
        // Values <= 1.0 are treated as relative for backward compatibility
        let x_coord = if x_config <= 1.0 && x_config >= 0.0 {
            CoordinateValue::Relative(x_config)
        } else {
            CoordinateValue::Absolute(x_config)
        };
        
        let y_coord = if y_config <= 1.0 && y_config >= 0.0 {
            CoordinateValue::Relative(y_config)
        } else {
            CoordinateValue::Absolute(y_config)
        };
        
        let w_coord = if w_config <= 1.0 && w_config >= 0.0 {
            CoordinateValue::Relative(w_config)
        } else {
            CoordinateValue::Absolute(w_config)
        };
        
        let h_coord = if h_config <= 1.0 && h_config >= 0.0 {
            CoordinateValue::Relative(h_config)
        } else {
            CoordinateValue::Absolute(h_config)
        };
        
        self.create_zone_rect_from_coordinates(screen_rect, x_coord, y_coord, w_coord, h_coord)
    }
    
    fn create_zone_rect_from_coordinates(
        &self, 
        screen_rect: Rect, 
        x_coord: CoordinateValue, 
        y_coord: CoordinateValue, 
        w_coord: CoordinateValue, 
        h_coord: CoordinateValue
    ) -> Rect {
        let x = screen_rect.x + x_coord.resolve(screen_rect.width);
        let y = screen_rect.y + y_coord.resolve(screen_rect.height);
        let width = w_coord.resolve(screen_rect.width);
        let height = h_coord.resolve(screen_rect.height);

        Rect::new(x, y, width, height)
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

            // Use configurable threshold for minimum drag distance
            let min_distance = self.snap_threshold;

            if drag_distance > min_distance {
                debug!("✅ Drag qualifies for processing, checking targets...");

                let center_x = final_rect.x + final_rect.width / 2.0;
                let center_y = final_rect.y + final_rect.height / 2.0;

                // Determine which zone we're in
                let current_zone = self.find_zone_at_point(center_x, center_y);

                match current_zone {
                    Some(SnapRegion::Center) => {
                        // In center swap zone - check for window to swap with
                        if let Some(target_window_id) =
                            self.find_window_under_drag(window_id, final_rect, all_windows)
                        {
                            debug!("🔄 Window dropped over another window in swap zone, initiating swap");
                            return DragResult::SwapWithWindow(
                                target_window_id,
                                drag_state.initial_rect,
                            );
                        } else {
                            debug!("📍 In center zone but no window to swap with, returning to original");
                            return DragResult::ReturnToOriginal(drag_state.initial_rect);
                        }
                    }
                    Some(
                        SnapRegion::North
                        | SnapRegion::South
                        | SnapRegion::East
                        | SnapRegion::West
                        | SnapRegion::NorthEast
                        | SnapRegion::NorthWest
                        | SnapRegion::SouthEast
                        | SnapRegion::SouthWest,
                    ) => {
                        // In warp/corner zone - snap to that zone regardless of other windows
                        if let Some(snap_rect) = self.find_snap_target(final_rect) {
                            debug!("🎯 Found warp/corner target: {:?}", snap_rect);
                            let dx = (snap_rect.x - final_rect.x).abs();
                            let dy = (snap_rect.y - final_rect.y).abs();
                            let dw = (snap_rect.width - final_rect.width).abs();
                            let dh = (snap_rect.height - final_rect.height).abs();

                            if dx > 10.0 || dy > 10.0 || dw > 10.0 || dh > 10.0 {
                                debug!("📌 Warping to zone");
                                return DragResult::SnapToZone(snap_rect);
                            } else {
                                debug!("↩️ Already close to warp target, returning to original");
                                return DragResult::ReturnToOriginal(drag_state.initial_rect);
                            }
                        } else {
                            debug!("🎯❌ No warp target found");
                            return DragResult::ReturnToOriginal(drag_state.initial_rect);
                        }
                    }
                    None => {
                        // Outside all zones - return to original position
                        debug!("🚫 Outside all snap zones, returning to original position");
                        return DragResult::ReturnToOriginal(drag_state.initial_rect);
                    }
                }
            } else {
                debug!(
                    "⏱️ Drag too small for processing (distance: {:.1}px < {:.1}px), returning to original",
                    drag_distance,
                    min_distance
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

    pub fn find_zone_at_point(&self, x: f64, y: f64) -> Option<SnapRegion> {
        // Check corners first (most specific)
        for zone in &self.snap_zones {
            if matches!(
                zone.region,
                SnapRegion::NorthWest
                    | SnapRegion::NorthEast
                    | SnapRegion::SouthWest
                    | SnapRegion::SouthEast
            ) && self.point_in_rect(x, y, &zone.bounds)
            {
                return Some(zone.region);
            }
        }

        // Then check edges
        for zone in &self.snap_zones {
            if matches!(
                zone.region,
                SnapRegion::North | SnapRegion::South | SnapRegion::East | SnapRegion::West
            ) && self.point_in_rect(x, y, &zone.bounds)
            {
                return Some(zone.region);
            }
        }

        // Finally check center
        for zone in &self.snap_zones {
            if zone.region == SnapRegion::Center && self.point_in_rect(x, y, &zone.bounds) {
                return Some(zone.region);
            }
        }

        None
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
