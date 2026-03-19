use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use eyre::Result;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, EventControllerKey, EventControllerMotion, GestureDrag};

use crate::config::Config;
use crate::rect::AtomicRect;

const APP_ID: &str = "com.scottidler.viewport2";

/// Determine which edge/corner the cursor is near, if any.
fn detect_edge(x: f64, y: f64, width: f64, height: f64, threshold: f64) -> Option<gdk::SurfaceEdge> {
    let near_left = x < threshold;
    let near_right = x > width - threshold;
    let near_top = y < threshold;
    let near_bottom = y > height - threshold;

    match (near_left, near_right, near_top, near_bottom) {
        (true, false, true, false) => Some(gdk::SurfaceEdge::NorthWest),
        (false, true, true, false) => Some(gdk::SurfaceEdge::NorthEast),
        (true, false, false, true) => Some(gdk::SurfaceEdge::SouthWest),
        (false, true, false, true) => Some(gdk::SurfaceEdge::SouthEast),
        (true, false, false, false) => Some(gdk::SurfaceEdge::West),
        (false, true, false, false) => Some(gdk::SurfaceEdge::East),
        (false, false, true, false) => Some(gdk::SurfaceEdge::North),
        (false, false, false, true) => Some(gdk::SurfaceEdge::South),
        _ => None,
    }
}

/// Map a surface edge to the appropriate CSS cursor name.
fn cursor_for_edge(edge: gdk::SurfaceEdge) -> &'static str {
    match edge {
        gdk::SurfaceEdge::NorthWest => "nw-resize",
        gdk::SurfaceEdge::North => "n-resize",
        gdk::SurfaceEdge::NorthEast => "ne-resize",
        gdk::SurfaceEdge::West => "w-resize",
        gdk::SurfaceEdge::East => "e-resize",
        gdk::SurfaceEdge::SouthWest => "sw-resize",
        gdk::SurfaceEdge::South => "s-resize",
        gdk::SurfaceEdge::SouthEast => "se-resize",
        _ => "default",
    }
}

pub fn run(config: &Config, shared_rect: Arc<AtomicRect>) -> Result<()> {
    let app = Application::builder().application_id(APP_ID).build();

    let config = config.clone();
    let rect = shared_rect.clone();

    app.connect_activate(move |app| {
        if let Err(e) = build_ui(app, &config, rect.clone()) {
            log::error!("Failed to build UI: {}", e);
        }
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_ui(app: &Application, config: &Config, shared_rect: Arc<AtomicRect>) -> Result<()> {
    let width = config.initial_size.width as i32;
    let height = config.initial_size.height as i32;
    let border_width = config.border_width;
    let border_color = &config.border_color;

    let css = format!(
        r#"
        window {{
            background-color: transparent;
        }}
        .viewport-frame {{
            border: {}px solid {};
            background-color: transparent;
            border-radius: 2px;
        }}
        .viewport-label {{
            color: {};
            background-color: rgba(0, 0, 0, 0.6);
            padding: 2px 6px;
            border-radius: 2px;
            font-size: 11px;
            font-family: monospace;
        }}
        "#,
        border_width, border_color, border_color
    );

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not get default display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .title("viewport2")
        .default_width(width)
        .default_height(height)
        .decorated(false)
        .resizable(true)
        .build();

    window.set_css_classes(&["viewport-frame"]);

    // Dimension label
    let label = gtk4::Label::new(Some(&format!("{}x{}", width, height)));
    label.set_css_classes(&["viewport-label"]);
    label.set_halign(gtk4::Align::End);
    label.set_valign(gtk4::Align::Start);
    label.set_margin_top(border_width as i32 + 4);
    label.set_margin_end(border_width as i32 + 4);

    let overlay_widget = gtk4::Overlay::new();
    overlay_widget.set_child(Some(&gtk4::Box::new(gtk4::Orientation::Vertical, 0)));
    overlay_widget.add_overlay(&label);
    window.set_child(Some(&overlay_widget));

    // Edge detection state shared between motion controller and drag gesture
    let edge_threshold = (border_width + 4).max(8) as f64;
    let current_edge: Rc<Cell<Option<gdk::SurfaceEdge>>> = Rc::new(Cell::new(None));

    // Motion controller: track cursor and update resize cursor
    let motion = EventControllerMotion::new();
    let win_motion = window.clone();
    let edge_for_motion = current_edge.clone();
    motion.connect_motion(move |_, x, y| {
        let w = win_motion.width() as f64;
        let h = win_motion.height() as f64;
        let edge = detect_edge(x, y, w, h, edge_threshold);
        edge_for_motion.set(edge);

        let cursor_name = match edge {
            Some(e) => cursor_for_edge(e),
            None => "default",
        };
        let cursor = gdk::Cursor::from_name(cursor_name, None);
        win_motion.set_cursor(cursor.as_ref());
    });
    window.add_controller(motion);

    // Edge-aware drag: resize from edges/corners, move from interior
    let drag = GestureDrag::new();
    drag.set_button(1);
    let win_drag = window.clone();
    let edge_for_drag = current_edge.clone();
    drag.connect_drag_begin(move |gesture, _, _| {
        if let Some(surface) = win_drag.surface()
            && let Some(toplevel) = surface.downcast_ref::<gdk::Toplevel>()
            && let Some(device) = gesture.device()
        {
            match edge_for_drag.get() {
                Some(edge) => {
                    toplevel.begin_resize(edge, Some(&device), 1, 0.0, 0.0, 0);
                }
                None => {
                    toplevel.begin_move(&device, 1, 0.0, 0.0, 0);
                }
            }
        }
    });
    window.add_controller(drag);

    // Keyboard shortcuts
    let key_controller = EventControllerKey::new();
    let win_key = window.clone();
    let rect_key = shared_rect.clone();
    let label_key = label.clone();
    let presets: Vec<(u32, u32)> = config.presets.iter().map(|s| (s.width, s.height)).collect();

    key_controller.connect_key_pressed(move |_, keyval, _, modifier| {
        let shift = modifier.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
        let ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
        let step = if shift { 1 } else { 10 };

        match keyval {
            gtk4::gdk::Key::Escape => {
                win_key.close();
                gtk4::glib::Propagation::Stop
            }
            // Ctrl+Arrow: resize
            gtk4::gdk::Key::Left if ctrl => {
                let r = rect_key.get();
                let new_w = r.width.saturating_sub(step);
                if new_w >= 100 {
                    rect_key.set_size(new_w, r.height);
                    win_key.set_default_size(new_w as i32, r.height as i32);
                    label_key.set_text(&format!("{}x{}", new_w, r.height));
                }
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Right if ctrl => {
                let r = rect_key.get();
                let new_w = r.width + step;
                rect_key.set_size(new_w, r.height);
                win_key.set_default_size(new_w as i32, r.height as i32);
                label_key.set_text(&format!("{}x{}", new_w, r.height));
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Up if ctrl => {
                let r = rect_key.get();
                let new_h = r.height.saturating_sub(step);
                if new_h >= 100 {
                    rect_key.set_size(r.width, new_h);
                    win_key.set_default_size(r.width as i32, new_h as i32);
                    label_key.set_text(&format!("{}x{}", r.width, new_h));
                }
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Down if ctrl => {
                let r = rect_key.get();
                let new_h = r.height + step;
                rect_key.set_size(r.width, new_h);
                win_key.set_default_size(r.width as i32, new_h as i32);
                label_key.set_text(&format!("{}x{}", r.width, new_h));
                gtk4::glib::Propagation::Stop
            }
            // Arrow keys (no ctrl): nudge crop position
            gtk4::gdk::Key::Left if !ctrl => {
                let r = rect_key.get();
                rect_key.set_position(r.x - step as i32, r.y);
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Right if !ctrl => {
                let r = rect_key.get();
                rect_key.set_position(r.x + step as i32, r.y);
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Up if !ctrl => {
                let r = rect_key.get();
                rect_key.set_position(r.x, r.y - step as i32);
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Down if !ctrl => {
                let r = rect_key.get();
                rect_key.set_position(r.x, r.y + step as i32);
                gtk4::glib::Propagation::Stop
            }
            // Preset sizes
            v @ (gtk4::gdk::Key::_1 | gtk4::gdk::Key::_2 | gtk4::gdk::Key::_3 | gtk4::gdk::Key::_4) => {
                let idx = match v {
                    gtk4::gdk::Key::_1 => 0,
                    gtk4::gdk::Key::_2 => 1,
                    gtk4::gdk::Key::_3 => 2,
                    _ => 3,
                };
                if let Some(&(w, h)) = presets.get(idx) {
                    rect_key.set_size(w, h);
                    win_key.set_default_size(w as i32, h as i32);
                    label_key.set_text(&format!("{}x{}", w, h));
                }
                gtk4::glib::Propagation::Stop
            }
            _ => gtk4::glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_controller);

    // Update shared rect when window is resized (width or height)
    let rect_resize_w = shared_rect.clone();
    let label_resize_w = label.clone();
    window.connect_default_width_notify(move |win| {
        let w = win.default_width() as u32;
        let h = win.default_height() as u32;
        rect_resize_w.set_size(w, h);
        label_resize_w.set_text(&format!("{}x{}", w, h));
    });

    let rect_resize_h = shared_rect.clone();
    let label_resize_h = label;
    window.connect_default_height_notify(move |win| {
        let w = win.default_width() as u32;
        let h = win.default_height() as u32;
        rect_resize_h.set_size(w, h);
        label_resize_h.set_text(&format!("{}x{}", w, h));
    });

    // Initialize shared rect
    shared_rect.set_size(width as u32, height as u32);

    window.present();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_edge_corners() {
        // 100x100 window, 8px threshold
        assert_eq!(
            detect_edge(2.0, 2.0, 100.0, 100.0, 8.0),
            Some(gdk::SurfaceEdge::NorthWest)
        );
        assert_eq!(
            detect_edge(98.0, 2.0, 100.0, 100.0, 8.0),
            Some(gdk::SurfaceEdge::NorthEast)
        );
        assert_eq!(
            detect_edge(2.0, 98.0, 100.0, 100.0, 8.0),
            Some(gdk::SurfaceEdge::SouthWest)
        );
        assert_eq!(
            detect_edge(98.0, 98.0, 100.0, 100.0, 8.0),
            Some(gdk::SurfaceEdge::SouthEast)
        );
    }

    #[test]
    fn test_detect_edge_sides() {
        assert_eq!(detect_edge(2.0, 50.0, 100.0, 100.0, 8.0), Some(gdk::SurfaceEdge::West));
        assert_eq!(detect_edge(98.0, 50.0, 100.0, 100.0, 8.0), Some(gdk::SurfaceEdge::East));
        assert_eq!(detect_edge(50.0, 2.0, 100.0, 100.0, 8.0), Some(gdk::SurfaceEdge::North));
        assert_eq!(
            detect_edge(50.0, 98.0, 100.0, 100.0, 8.0),
            Some(gdk::SurfaceEdge::South)
        );
    }

    #[test]
    fn test_detect_edge_interior() {
        assert_eq!(detect_edge(50.0, 50.0, 100.0, 100.0, 8.0), None);
        assert_eq!(detect_edge(20.0, 20.0, 100.0, 100.0, 8.0), None);
        assert_eq!(detect_edge(80.0, 80.0, 100.0, 100.0, 8.0), None);
    }
}
