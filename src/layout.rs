use crate::config::{GeneralConfig, LayoutConfig};
use crate::{Rect, Window, WindowId};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum LayoutType {
    BSP,
    Stack,
    Float,
    Grid,
    Spiral,
    Column,
    Monocle,
}

impl LayoutType {
    fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bsp" | "binary" => Self::BSP,
            "stack" | "stacking" => Self::Stack,
            "float" | "floating" => Self::Float,
            "grid" => Self::Grid,
            "spiral" => Self::Spiral,
            "column" | "columns" => Self::Column,
            "monocle" | "fullscreen" => Self::Monocle,
            _ => Self::BSP,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::BSP => "BSP",
            Self::Stack => "Stack",
            Self::Float => "Float",
            Self::Grid => "Grid",
            Self::Spiral => "Spiral",
            Self::Column => "Column",
            Self::Monocle => "Monocle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BSPNode {
    pub rect: Rect,
    pub split_ratio: f64,
    pub is_horizontal: bool,
    pub window_id: Option<WindowId>,
    pub left: Option<Box<BSPNode>>,
    pub right: Option<Box<BSPNode>>,
}

impl BSPNode {
    pub fn new_leaf(window_id: WindowId, rect: Rect) -> Self {
        Self {
            rect,
            split_ratio: 0.5,
            is_horizontal: true,
            window_id: Some(window_id),
            left: None,
            right: None,
        }
    }

    pub fn new_container(rect: Rect, is_horizontal: bool, split_ratio: f64) -> Self {
        Self {
            rect,
            split_ratio,
            is_horizontal,
            window_id: None,
            left: None,
            right: None,
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }

    pub fn insert_window(&mut self, window_id: WindowId, split_ratio: f64) {
        if self.is_leaf() {
            if let Some(existing_id) = self.window_id {
                let left_rect = if self.is_horizontal {
                    Rect::new(
                        self.rect.x,
                        self.rect.y,
                        self.rect.width * self.split_ratio,
                        self.rect.height,
                    )
                } else {
                    Rect::new(
                        self.rect.x,
                        self.rect.y,
                        self.rect.width,
                        self.rect.height * self.split_ratio,
                    )
                };

                let right_rect = if self.is_horizontal {
                    Rect::new(
                        self.rect.x + left_rect.width,
                        self.rect.y,
                        self.rect.width - left_rect.width,
                        self.rect.height,
                    )
                } else {
                    Rect::new(
                        self.rect.x,
                        self.rect.y + left_rect.height,
                        self.rect.width,
                        self.rect.height - left_rect.height,
                    )
                };

                self.left = Some(Box::new(BSPNode::new_leaf(existing_id, left_rect)));
                self.right = Some(Box::new(BSPNode::new_leaf(window_id, right_rect)));
                self.window_id = None;
                self.split_ratio = split_ratio;
            } else {
                self.window_id = Some(window_id);
            }
        } else {
            if let Some(ref mut right) = self.right {
                right.insert_window(window_id, split_ratio);
            }
        }
    }

    pub fn collect_window_rects(&self, gap: f64) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();
        self.collect_rects_recursive(&mut rects, gap);
        rects
    }

    fn collect_rects_recursive(&self, rects: &mut HashMap<WindowId, Rect>, gap: f64) {
        if let Some(window_id) = self.window_id {
            let adjusted_rect = Rect::new(
                self.rect.x + gap / 2.0,
                self.rect.y + gap / 2.0,
                self.rect.width - gap,
                self.rect.height - gap,
            );
            rects.insert(window_id, adjusted_rect);
        } else {
            if let Some(ref left) = self.left {
                left.collect_rects_recursive(rects, gap);
            }
            if let Some(ref right) = self.right {
                right.collect_rects_recursive(rects, gap);
            }
        }
    }
}

pub struct LayoutManager {
    current_layout: LayoutType,
    bsp_root: Option<BSPNode>,
    split_ratio: f64,
}

impl LayoutManager {
    pub fn new(config: &LayoutConfig) -> Self {
        Self {
            current_layout: LayoutType::from_string(&config.default_layout),
            bsp_root: None,
            split_ratio: config.split_ratio,
        }
    }

    pub fn compute_layout(
        &mut self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        match self.current_layout {
            LayoutType::BSP => self.compute_bsp_layout(windows, screen_rect, general_config),
            LayoutType::Stack => self.compute_stack_layout(windows, screen_rect, general_config),
            LayoutType::Float => self.compute_float_layout(windows, screen_rect, general_config),
            LayoutType::Grid => self.compute_grid_layout(windows, screen_rect, general_config),
            LayoutType::Spiral => self.compute_spiral_layout(windows, screen_rect, general_config),
            LayoutType::Column => self.compute_column_layout(windows, screen_rect, general_config),
            LayoutType::Monocle => {
                self.compute_monocle_layout(windows, screen_rect, general_config)
            }
        }
    }

    fn compute_bsp_layout(
        &mut self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        if windows.is_empty() {
            return HashMap::new();
        }

        let mut root = BSPNode::new_leaf(windows[0].id, screen_rect);

        for window in windows.iter().skip(1) {
            root.insert_window(window.id, self.split_ratio);
        }

        self.bsp_root = Some(root.clone());
        root.collect_window_rects(general_config.gap)
    }

    fn compute_stack_layout(
        &self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();

        if windows.is_empty() {
            return rects;
        }

        if windows.len() == 1 {
            let adjusted_rect = Rect::new(
                screen_rect.x + general_config.gap,
                screen_rect.y + general_config.gap,
                screen_rect.width - 2.0 * general_config.gap,
                screen_rect.height - 2.0 * general_config.gap,
            );
            rects.insert(windows[0].id, adjusted_rect);
            return rects;
        }

        let master_width = screen_rect.width * self.split_ratio;
        let stack_width = screen_rect.width - master_width;
        let stack_height = screen_rect.height / (windows.len() - 1) as f64;

        let master_rect = Rect::new(
            screen_rect.x + general_config.gap / 2.0,
            screen_rect.y + general_config.gap / 2.0,
            master_width - general_config.gap,
            screen_rect.height - general_config.gap,
        );
        rects.insert(windows[0].id, master_rect);

        for (i, window) in windows.iter().skip(1).enumerate() {
            let stack_rect = Rect::new(
                screen_rect.x + master_width + general_config.gap / 2.0,
                screen_rect.y + i as f64 * stack_height + general_config.gap / 2.0,
                stack_width - general_config.gap,
                stack_height - general_config.gap,
            );
            rects.insert(window.id, stack_rect);
        }

        rects
    }

    fn compute_float_layout(
        &self,
        windows: &[&Window],
        _screen_rect: Rect,
        _general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        windows.iter().map(|w| (w.id, w.rect.clone())).collect()
    }

    fn compute_grid_layout(
        &self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();

        if windows.is_empty() {
            return rects;
        }

        let window_count = windows.len();
        let cols = (window_count as f64).sqrt().ceil() as usize;
        let rows = (window_count + cols - 1) / cols;

        let cell_width = (screen_rect.width - general_config.gap * (cols + 1) as f64) / cols as f64;
        let cell_height =
            (screen_rect.height - general_config.gap * (rows + 1) as f64) / rows as f64;

        for (i, window) in windows.iter().enumerate() {
            let row = i / cols;
            let col = i % cols;

            let x =
                screen_rect.x + general_config.gap + col as f64 * (cell_width + general_config.gap);
            let y = screen_rect.y
                + general_config.gap
                + row as f64 * (cell_height + general_config.gap);

            let rect = Rect::new(x, y, cell_width, cell_height);
            rects.insert(window.id, rect);
        }

        rects
    }

    fn compute_spiral_layout(
        &self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();

        if windows.is_empty() {
            return rects;
        }

        if windows.len() == 1 {
            let rect = Rect::new(
                screen_rect.x + general_config.gap,
                screen_rect.y + general_config.gap,
                screen_rect.width - 2.0 * general_config.gap,
                screen_rect.height - 2.0 * general_config.gap,
            );
            rects.insert(windows[0].id, rect);
            return rects;
        }

        // Spiral layout: first window takes half the screen, others spiral around
        let main_rect = Rect::new(
            screen_rect.x + general_config.gap / 2.0,
            screen_rect.y + general_config.gap / 2.0,
            screen_rect.width * self.split_ratio - general_config.gap,
            screen_rect.height - general_config.gap,
        );
        rects.insert(windows[0].id, main_rect);

        if windows.len() > 1 {
            let side_width = screen_rect.width * (1.0 - self.split_ratio);
            let side_height_per_window = screen_rect.height / (windows.len() - 1) as f64;

            for (i, window) in windows.iter().skip(1).enumerate() {
                let rect = Rect::new(
                    screen_rect.x + screen_rect.width * self.split_ratio + general_config.gap / 2.0,
                    screen_rect.y + i as f64 * side_height_per_window + general_config.gap / 2.0,
                    side_width - general_config.gap,
                    side_height_per_window - general_config.gap,
                );
                rects.insert(window.id, rect);
            }
        }

        rects
    }

    fn compute_column_layout(
        &self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();

        if windows.is_empty() {
            return rects;
        }

        let window_width = (screen_rect.width - general_config.gap * (windows.len() + 1) as f64)
            / windows.len() as f64;

        for (i, window) in windows.iter().enumerate() {
            let x =
                screen_rect.x + general_config.gap + i as f64 * (window_width + general_config.gap);
            let y = screen_rect.y + general_config.gap;
            let height = screen_rect.height - 2.0 * general_config.gap;

            let rect = Rect::new(x, y, window_width, height);
            rects.insert(window.id, rect);
        }

        rects
    }

    fn compute_monocle_layout(
        &self,
        windows: &[&Window],
        screen_rect: Rect,
        general_config: &GeneralConfig,
    ) -> HashMap<WindowId, Rect> {
        let mut rects = HashMap::new();

        if windows.is_empty() {
            return rects;
        }

        // In monocle mode, all windows are fullscreen (only focused one is visible)
        let fullscreen_rect = Rect::new(
            screen_rect.x + general_config.gap,
            screen_rect.y + general_config.gap,
            screen_rect.width - 2.0 * general_config.gap,
            screen_rect.height - 2.0 * general_config.gap,
        );

        for window in windows {
            rects.insert(window.id, fullscreen_rect.clone());
        }

        rects
    }

    pub fn toggle_layout(&mut self) {
        self.current_layout = match self.current_layout {
            LayoutType::BSP => LayoutType::Stack,
            LayoutType::Stack => LayoutType::Grid,
            LayoutType::Grid => LayoutType::Spiral,
            LayoutType::Spiral => LayoutType::Column,
            LayoutType::Column => LayoutType::Monocle,
            LayoutType::Monocle => LayoutType::Float,
            LayoutType::Float => LayoutType::BSP,
        };
    }

    pub fn adjust_split_ratio(&mut self, delta: f64) {
        self.split_ratio = (self.split_ratio + delta).max(0.1).min(0.9);
    }

    pub fn get_split_ratio(&self) -> f64 {
        self.split_ratio
    }

    pub fn reset_split_ratio(&mut self) {
        self.split_ratio = 0.5;
    }

    pub fn next_layout(&mut self) {
        self.toggle_layout();
    }

    pub fn previous_layout(&mut self) {
        // Cycle backwards through layouts
        self.current_layout = match self.current_layout {
            LayoutType::BSP => LayoutType::Float,
            LayoutType::Stack => LayoutType::BSP,
            LayoutType::Grid => LayoutType::Stack,
            LayoutType::Spiral => LayoutType::Grid,
            LayoutType::Column => LayoutType::Spiral,
            LayoutType::Monocle => LayoutType::Column,
            LayoutType::Float => LayoutType::Monocle,
        };
    }

    pub fn set_layout(&mut self, layout: LayoutType) {
        self.current_layout = layout;
    }

    pub fn current_layout(&self) -> &LayoutType {
        &self.current_layout
    }
}
