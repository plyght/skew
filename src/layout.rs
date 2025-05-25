use crate::config::{GeneralConfig, LayoutConfig};
use crate::{Rect, Window, WindowId};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum LayoutType {
    BSP,
    Stack,
    Float,
}

impl LayoutType {
    fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bsp" | "binary" => Self::BSP,
            "stack" | "stacking" => Self::Stack,
            "float" | "floating" => Self::Float,
            _ => Self::BSP,
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

    pub fn toggle_layout(&mut self) {
        self.current_layout = match self.current_layout {
            LayoutType::BSP => LayoutType::Stack,
            LayoutType::Stack => LayoutType::Float,
            LayoutType::Float => LayoutType::BSP,
        };
    }

    pub fn set_layout(&mut self, layout: LayoutType) {
        self.current_layout = layout;
    }

    pub fn current_layout(&self) -> &LayoutType {
        &self.current_layout
    }
}
