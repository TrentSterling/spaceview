use eframe::egui;

const MAX_ZOOM: f32 = 5_000.0;
const MIN_ZOOM: f32 = 1.0;

/// Continuous camera for world-space treemap viewing.
/// Supports smooth scroll-zoom, click-drag pan, and snap-zoom animations.
pub struct Camera {
    pub center: egui::Pos2,
    pub zoom: f32,
    pub target_center: egui::Pos2,
    pub target_zoom: f32,
    // Snap-zoom animation
    anim_start_center: egui::Pos2,
    anim_start_zoom: f32,
    anim_progress: f32,
    animating: bool,
    // World bounds
    world_rect: egui::Rect,
}

/// Ease-out cubic: fast start, smooth deceleration
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn clamp_point(center: &mut egui::Pos2, zoom: f32, viewport: egui::Rect, wr: egui::Rect) {
    let half_w = 0.5 / zoom;
    let half_h = (viewport.height() / viewport.width()) * 0.5 / zoom;

    // X axis
    if wr.width() <= half_w * 2.0 {
        center.x = wr.center().x;
    } else {
        center.x = center.x.clamp(wr.min.x + half_w, wr.max.x - half_w);
    }

    // Y axis
    if wr.height() <= half_h * 2.0 {
        center.y = wr.center().y;
    } else {
        center.y = center.y.clamp(wr.min.y + half_h, wr.max.y - half_h);
    }
}

const SNAP_DURATION: f32 = 0.35; // seconds
const SCROLL_ZOOM_SPEED: f32 = 0.15;
const PAN_SMOOTHING: f32 = 0.25; // exponential lerp factor per tick. Lower = smoother
const ZOOM_SMOOTHING: f32 = 0.20;

impl Camera {
    pub fn new(center: egui::Pos2, zoom: f32) -> Self {
        Self {
            center,
            zoom,
            target_center: center,
            target_zoom: zoom,
            anim_start_center: center,
            anim_start_zoom: zoom,
            anim_progress: 0.0,
            animating: false,
            world_rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        }
    }

    /// Set the world bounds for camera clamping.
    pub fn set_world_rect(&mut self, rect: egui::Rect) {
        self.world_rect = rect;
    }

    /// Whether the camera is currently animating a snap-to.
    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Reset camera to show the full world rect.
    pub fn reset(&mut self, world_rect: egui::Rect) {
        let c = world_rect.center();
        self.center = c;
        self.zoom = 1.0;
        self.target_center = c;
        self.target_zoom = 1.0;
        self.animating = false;
        self.world_rect = world_rect;
    }

    /// Transform a world-space rect to screen-space.
    pub fn world_to_screen(&self, world: egui::Rect, viewport: egui::Rect) -> egui::Rect {
        let vp_center = viewport.center();
        let scale = self.zoom * viewport.width();
        let min = egui::pos2(
            (world.min.x - self.center.x) * scale + vp_center.x,
            (world.min.y - self.center.y) * scale + vp_center.y,
        );
        let max = egui::pos2(
            (world.max.x - self.center.x) * scale + vp_center.x,
            (world.max.y - self.center.y) * scale + vp_center.y,
        );
        egui::Rect::from_min_max(min, max)
    }

    /// Transform a screen position to world-space.
    pub fn screen_to_world(&self, screen_pos: egui::Pos2, viewport: egui::Rect) -> egui::Pos2 {
        let vp_center = viewport.center();
        let scale = self.zoom * viewport.width();
        egui::pos2(
            (screen_pos.x - vp_center.x) / scale + self.center.x,
            (screen_pos.y - vp_center.y) / scale + self.center.y,
        )
    }

    /// Clamp center so the viewport always stays within world_rect.
    /// If the world is smaller than the viewport in a dimension, center it.
    fn clamp_center(&mut self, viewport: egui::Rect) {
        let wr = self.world_rect; // copy to avoid borrow conflicts

        clamp_point(&mut self.target_center, self.target_zoom, viewport, wr);
        clamp_point(&mut self.center, self.zoom, viewport, wr);
    }

    /// Advance animations. Call once per frame.
    /// Returns true if the camera is still moving (request_repaint needed).
    pub fn tick(&mut self, dt: f32, viewport: egui::Rect) -> bool {
        if self.animating {
            self.anim_progress += dt / SNAP_DURATION;
            if self.anim_progress >= 1.0 {
                self.anim_progress = 1.0;
                self.animating = false;
                self.center = self.target_center;
                self.zoom = self.target_zoom;
            } else {
                let t = ease_out_cubic(self.anim_progress);
                self.center = egui::pos2(
                    self.anim_start_center.x + (self.target_center.x - self.anim_start_center.x) * t,
                    self.anim_start_center.y + (self.target_center.y - self.anim_start_center.y) * t,
                );
                self.zoom = self.anim_start_zoom + (self.target_zoom - self.anim_start_zoom) * t;
            }
            self.clamp_center(viewport);
            return true;
        }

        // Exponential lerp toward targets (scroll zoom / residual pan)
        let mut moving = false;

        let zoom_diff = (self.target_zoom - self.zoom).abs();
        if zoom_diff > 0.001 {
            let factor = 1.0 - (-ZOOM_SMOOTHING / dt.max(0.001)).exp();
            self.zoom += (self.target_zoom - self.zoom) * factor.min(1.0);
            moving = true;
        } else if zoom_diff > 0.0 {
            self.zoom = self.target_zoom;
        }

        let cx_diff = (self.target_center.x - self.center.x).abs();
        let cy_diff = (self.target_center.y - self.center.y).abs();
        if cx_diff > 0.00001 || cy_diff > 0.00001 {
            let factor = 1.0 - (-PAN_SMOOTHING / dt.max(0.001)).exp();
            let f = factor.min(1.0);
            self.center.x += (self.target_center.x - self.center.x) * f;
            self.center.y += (self.target_center.y - self.center.y) * f;
            moving = true;
        } else {
            self.center = self.target_center;
        }

        if moving {
            self.clamp_center(viewport);
        }

        moving
    }

    /// Scroll-zoom centered on a world point (the point under cursor stays fixed).
    pub fn scroll_zoom(&mut self, scroll_delta: f32, world_focus: egui::Pos2, viewport: egui::Rect) {
        // Interrupt snap animation. User takes manual control
        if self.animating {
            self.animating = false;
        }

        let factor = (1.0 + SCROLL_ZOOM_SPEED).powf(scroll_delta);
        let new_zoom = (self.target_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);

        // Adjust center so that world_focus stays at the same screen position.
        let old_zoom = self.target_zoom;
        let ratio = old_zoom / new_zoom;
        self.target_center = egui::pos2(
            world_focus.x - (world_focus.x - self.target_center.x) * ratio,
            world_focus.y - (world_focus.y - self.target_center.y) * ratio,
        );
        self.target_zoom = new_zoom;
        self.clamp_center(viewport);
    }

    /// Immediate pan by a world-space delta.
    pub fn drag_pan(&mut self, world_delta: egui::Vec2, viewport: egui::Rect) {
        if self.animating {
            self.animating = false;
        }
        self.target_center -= world_delta;
        // Snap directly for responsive dragging
        self.center = self.target_center;
        self.clamp_center(viewport);
    }

    /// Animated snap-zoom so that `world_rect` fills the viewport.
    pub fn snap_to(&mut self, world_rect: egui::Rect, viewport: egui::Rect) {
        self.anim_start_center = self.center;
        self.anim_start_zoom = self.zoom;

        self.target_center = world_rect.center();
        // Zoom so the rect fills the viewport (fit shorter axis)
        // zoom * vp_w * world_w = vp_w → zoom = 1/world_w
        let zoom_w = 1.0 / world_rect.width();
        // zoom * vp_w * world_h = vp_h → zoom = vp_h / (vp_w * world_h)
        let zoom_h = viewport.height() / (world_rect.height() * viewport.width());
        self.target_zoom = zoom_w.min(zoom_h).clamp(MIN_ZOOM, MAX_ZOOM);

        self.clamp_center(viewport);

        self.anim_progress = 0.0;
        self.animating = true;
    }
}
