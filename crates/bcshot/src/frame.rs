//! The on-screen frame, Bandicam style.
//!
//! * Locked (normal use): a small borderless, always-on-top, click-through
//!   window that just draws the blue border OUTSIDE the capture area, so it
//!   never appears in the screenshot. It stays where it is between captures
//!   and across restarts (the rectangle lives in the config file).
//! * Adjusting: the window grows to cover the monitor and takes the mouse:
//!   drag inside = move, drag an edge/corner = resize, drag outside = draw a
//!   new one. Arrow keys nudge. Enter / double-click / "Lock" saves; Esc undoes.
//!
//! All geometry is converted through "global points" (the desktop coordinate
//! system egui positions windows in), so it is independent of 100/125/150 %
//! scaling and of how GNOME/KDE scale XWayland.

use crate::server::Shared;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
    ViewportBuilder, ViewportCommand, ViewportId,
};
use std::sync::{Arc, Mutex};

const BLUE: Color32 = Color32::from_rgb(41, 92, 157); // Bandicam's frame colour
const BORDER: f32 = 3.0; // locked border thickness, points
const GRAB: f32 = 8.0; // how close to an edge counts as "on the edge", points
const MIN_PX: u32 = 8;

#[derive(Clone, Copy)]
enum Kind {
    Move,
    Resize { l: bool, r: bool, t: bool, b: bool },
    New,
}

struct Drag {
    kind: Kind,
    start_ptr: Pos2,  // global points
    start_rect: Rect, // global points
}

#[derive(Default)]
pub struct FrameEditor {
    /// rectangle before adjusting started (for Esc); Some == adjusting
    orig: Option<[u32; 4]>,
    drag: Option<Drag>,
    need_focus: bool,
    passthrough: Option<bool>,
    /// last window geometry we asked for (global points)
    last_win: Option<Rect>,
}

impl FrameEditor {
    pub fn unlock(&mut self, s: &mut Shared, cfg: &bcshot_common::Config) {
        self.orig = Some([cfg.x, cfg.y, cfg.width, cfg.height]);
        self.drag = None;
        self.need_focus = true;
        s.editing = true;
    }

    pub fn lock(&mut self, s: &mut Shared) {
        self.orig = None;
        self.drag = None;
        s.editing = false;
        let _ = s.cfg.save(); // the frame stays here from now on
    }

    fn cancel(&mut self, s: &mut Shared) {
        if let Some([x, y, w, h]) = self.orig {
            (s.cfg.x, s.cfg.y, s.cfg.width, s.cfg.height) = (x, y, w, h);
        }
        self.lock(s);
    }

    pub fn show(&mut self, ctx: &egui::Context, shared: &Arc<Mutex<Shared>>) {
        let (cfg, editing) = {
            let s = shared.lock().unwrap();
            (s.cfg.clone(), s.editing)
        };

        // Monitor size in points, from the main window's monitor.
        let ppp = ctx.native_pixels_per_point().unwrap_or(1.0);
        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(cfg.ref_w as f32 / ppp, cfg.ref_h as f32 / ppp));
        // screen pixels -> global points
        let k = egui::vec2(monitor.x / cfg.ref_w as f32, monitor.y / cfg.ref_h as f32);
        let to_pts = |px: [u32; 4]| {
            Rect::from_min_size(
                egui::pos2(px[0] as f32 * k.x, px[1] as f32 * k.y),
                egui::vec2(px[2] as f32 * k.x, px[3] as f32 * k.y),
            )
        };
        let to_px = |r: Rect| -> [u32; 4] {
            let x = ((r.min.x / k.x).round().max(0.0) as u32).min(cfg.ref_w - MIN_PX);
            let y = ((r.min.y / k.y).round().max(0.0) as u32).min(cfg.ref_h - MIN_PX);
            let w = ((r.width() / k.x).round().max(0.0) as u32).clamp(MIN_PX, cfg.ref_w - x);
            let h = ((r.height() / k.y).round().max(0.0) as u32).clamp(MIN_PX, cfg.ref_h - y);
            [x, y, w, h]
        };
        let screen = Rect::from_min_size(Pos2::ZERO, monitor); // global points
        let region = to_pts([cfg.x, cfg.y, cfg.width, cfg.height]);

        // Window placement: around the region when locked, whole monitor when
        // adjusting, a 1x1 invisible speck when the frame is hidden.
        let win = if editing {
            screen
        } else if cfg.show_frame {
            region.expand(BORDER)
        } else {
            Rect::from_min_size(Pos2::ZERO, egui::vec2(1.0, 1.0))
        };
        if self.last_win != Some(win) {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(win.min));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(win.size()));
            self.last_win = Some(win);
            ctx.request_repaint(); // redraw once the WM has moved us
        }

        let mut action: Option<fn(&mut FrameEditor, &mut Shared)> = None;
        let mut new_rect: Option<[u32; 4]> = None;

        if self.passthrough != Some(!editing) {
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(!editing));
            self.passthrough = Some(!editing);
        }
        // Where the window actually ended up (the WM may have nudged it).
        let origin = ctx
            .input(|i| i.viewport().inner_rect.map(|r| r.min))
            .unwrap_or(win.min)
            .to_vec2();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let painter = ui.painter().clone();
                let local = |r: Rect| r.translate(-origin);

                if !editing {
                    if !cfg.show_frame {
                        return;
                    }
                    painter.rect_stroke(
                        local(region).expand(BORDER / 2.0),
                        0.0,
                        Stroke::new(BORDER, BLUE),
                        StrokeKind::Middle,
                    );
                    return;
                }

                if self.need_focus {
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                    self.need_focus = false;
                }

                // ---------- mouse ----------
                let resp =
                    ui.interact(ui.max_rect(), ui.id().with("edit"), Sense::click_and_drag());
                let mut rect = region;

                if let Some(h) = resp.hover_pos() {
                    let kind = self
                        .drag
                        .as_ref()
                        .map(|d| d.kind)
                        .unwrap_or_else(|| zone(h + origin, rect));
                    ctx.set_cursor_icon(cursor_for(kind));
                }
                if resp.drag_started() {
                    let p = ctx
                        .input(|i| i.pointer.press_origin())
                        .or(resp.interact_pointer_pos());
                    if let Some(p) = p {
                        let p = p + origin;
                        self.drag = Some(Drag {
                            kind: zone(p, rect),
                            start_ptr: p,
                            start_rect: rect,
                        });
                    }
                }
                if let (Some(d), Some(cur)) = (&self.drag, resp.interact_pointer_pos()) {
                    if resp.dragged() {
                        let cur = cur + origin;
                        let delta = cur - d.start_ptr;
                        rect = match d.kind {
                            Kind::Move => {
                                let r = d.start_rect.translate(delta);
                                let min = egui::pos2(
                                    r.min.x.clamp(screen.min.x, screen.max.x - r.width()),
                                    r.min.y.clamp(screen.min.y, screen.max.y - r.height()),
                                );
                                Rect::from_min_size(min, r.size())
                            }
                            Kind::Resize { l, r, t, b } => {
                                let mut a = d.start_rect;
                                if l {
                                    a.min.x += delta.x
                                }
                                if r {
                                    a.max.x += delta.x
                                }
                                if t {
                                    a.min.y += delta.y
                                }
                                if b {
                                    a.max.y += delta.y
                                }
                                Rect::from_two_pos(a.min, a.max).intersect(screen)
                            }
                            Kind::New => Rect::from_two_pos(d.start_ptr, cur).intersect(screen),
                        };
                        new_rect = Some(to_px(rect));
                    }
                }
                if resp.drag_stopped() {
                    self.drag = None;
                }
                if resp.double_clicked()
                    && region.contains(resp.interact_pointer_pos().unwrap_or_default() + origin)
                {
                    action = Some(FrameEditor::lock);
                }

                // ---------- keyboard ----------
                ctx.input(|i| {
                    let step = if i.modifiers.shift { 10 } else { 1 };
                    let mut px = new_rect.unwrap_or([cfg.x, cfg.y, cfg.width, cfg.height]);
                    let mut changed = false;
                    for (key, dx, dy) in [
                        (Key::ArrowLeft, -1i64, 0i64),
                        (Key::ArrowRight, 1, 0),
                        (Key::ArrowUp, 0, -1),
                        (Key::ArrowDown, 0, 1),
                    ] {
                        if !i.key_pressed(key) {
                            continue;
                        }
                        changed = true;
                        let (dx, dy) = (dx * step, dy * step);
                        if i.modifiers.ctrl {
                            px[2] = (px[2] as i64 + dx)
                                .clamp(MIN_PX as i64, (cfg.ref_w - px[0]) as i64)
                                as u32;
                            px[3] = (px[3] as i64 + dy)
                                .clamp(MIN_PX as i64, (cfg.ref_h - px[1]) as i64)
                                as u32;
                        } else {
                            px[0] = (px[0] as i64 + dx).clamp(0, (cfg.ref_w - px[2]) as i64) as u32;
                            px[1] = (px[1] as i64 + dy).clamp(0, (cfg.ref_h - px[3]) as i64) as u32;
                        }
                    }
                    if changed {
                        new_rect = Some(px);
                    }
                    if i.key_pressed(Key::Enter) {
                        action = Some(FrameEditor::lock);
                    }
                    if i.key_pressed(Key::Escape) || i.viewport().close_requested() {
                        action = Some(FrameEditor::cancel);
                    }
                });

                // ---------- drawing ----------
                let shown = local(new_rect.map(to_pts).unwrap_or(rect));
                let full = local(screen);
                let dim = Color32::from_black_alpha(110);
                for r in [
                    Rect::from_min_max(full.min, egui::pos2(full.max.x, shown.min.y)),
                    Rect::from_min_max(egui::pos2(full.min.x, shown.max.y), full.max),
                    Rect::from_min_max(
                        egui::pos2(full.min.x, shown.min.y),
                        egui::pos2(shown.min.x, shown.max.y),
                    ),
                    Rect::from_min_max(
                        egui::pos2(shown.max.x, shown.min.y),
                        egui::pos2(full.max.x, shown.max.y),
                    ),
                ] {
                    painter.rect_filled(r, 0.0, dim);
                }
                // nearly invisible fill so clicks inside the frame land on this window
                painter.rect_filled(shown, 0.0, Color32::from_black_alpha(1));
                painter.rect_stroke(shown, 0.0, Stroke::new(2.0, BLUE), StrokeKind::Outside);
                for p in handles(shown) {
                    let h = Rect::from_center_size(p, Vec2::splat(8.0));
                    painter.rect_filled(h, 0.0, Color32::WHITE);
                    painter.rect_stroke(h, 0.0, Stroke::new(1.0, BLUE), StrokeKind::Inside);
                }

                // ---------- toolbar (size + Lock / Cancel) ----------
                let px = new_rect.unwrap_or([cfg.x, cfg.y, cfg.width, cfg.height]);
                let mut tb = Rect::from_min_size(
                    shown.left_top() - egui::vec2(0.0, 32.0),
                    egui::vec2(280.0, 28.0),
                );
                if tb.top() < full.top() {
                    tb = tb.translate(egui::vec2(0.0, 36.0)); // no room above: put it inside
                }
                if tb.right() > full.right() {
                    tb = tb.translate(egui::vec2(full.right() - tb.right(), 0.0));
                }
                painter.rect_filled(tb, 4.0, Color32::from_rgb(30, 30, 36));
                painter.text(
                    tb.left_center() + egui::vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    format!("{}x{}", px[2], px[3]),
                    FontId::proportional(15.0),
                    Color32::WHITE,
                );
                let b1 = Rect::from_min_size(
                    tb.right_top() + egui::vec2(-176.0, 3.0),
                    egui::vec2(84.0, 22.0),
                );
                let b2 = b1.translate(egui::vec2(88.0, 0.0));
                if ui.put(b1, egui::Button::new("Lock (Enter)")).clicked() {
                    action = Some(FrameEditor::lock);
                }
                if ui.put(b2, egui::Button::new("Cancel (Esc)")).clicked() {
                    action = Some(FrameEditor::cancel);
                }
            });

        let mut s = shared.lock().unwrap();
        if let Some([x, y, w, h]) = new_rect {
            (s.cfg.x, s.cfg.y, s.cfg.width, s.cfg.height) = (x, y, w, h);
            ctx.request_repaint();
        }
        if let Some(f) = action {
            f(self, &mut s);
            ctx.request_repaint();
        }
    }
}

/// What dragging from point `p` would do to rectangle `r` (global points).
fn zone(p: Pos2, r: Rect) -> Kind {
    let in_y = p.y >= r.top() - GRAB && p.y <= r.bottom() + GRAB;
    let in_x = p.x >= r.left() - GRAB && p.x <= r.right() + GRAB;
    let l = in_y && (p.x - r.left()).abs() <= GRAB;
    let rr = in_y && (p.x - r.right()).abs() <= GRAB && !l;
    let t = in_x && (p.y - r.top()).abs() <= GRAB;
    let b = in_x && (p.y - r.bottom()).abs() <= GRAB && !t;
    if l || rr || t || b {
        Kind::Resize { l, r: rr, t, b }
    } else if r.contains(p) {
        Kind::Move
    } else {
        Kind::New
    }
}

fn cursor_for(k: Kind) -> CursorIcon {
    match k {
        Kind::Move => CursorIcon::Move,
        Kind::New => CursorIcon::Crosshair,
        Kind::Resize { l, r, t, b } => match (l || r, t || b) {
            (true, true) if (l && t) || (r && b) => CursorIcon::ResizeNwSe,
            (true, true) => CursorIcon::ResizeNeSw,
            (true, false) => CursorIcon::ResizeHorizontal,
            _ => CursorIcon::ResizeVertical,
        },
    }
}

fn handles(r: Rect) -> [Pos2; 8] {
    let c = r.center();
    [
        r.left_top(),
        egui::pos2(c.x, r.top()),
        r.right_top(),
        egui::pos2(r.right(), c.y),
        r.right_bottom(),
        egui::pos2(c.x, r.bottom()),
        r.left_bottom(),
        egui::pos2(r.left(), c.y),
    ]
}
