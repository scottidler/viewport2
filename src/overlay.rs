use std::sync::Arc;

use eyre::Result;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, EventControllerKey, GestureDrag};

use crate::config::Config;
use crate::rect::AtomicRect;

const APP_ID: &str = "com.scottidler.viewport2";

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

    // Window dragging via surface begin_move (works on Wayland)
    let drag = GestureDrag::new();
    drag.set_button(1);
    let win_drag = window.clone();

    drag.connect_drag_begin(move |gesture, _, _| {
        if let Some(surface) = win_drag.surface()
            && let Some(toplevel) = surface.downcast_ref::<gtk4::gdk::Toplevel>()
            && let Some(device) = gesture.device()
        {
            toplevel.begin_move(&device, 1, 0.0, 0.0, 0);
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

    // Update shared rect when window is resized
    let rect_resize = shared_rect.clone();
    let label_resize = label;
    window.connect_default_width_notify(move |win| {
        let w = win.default_width() as u32;
        let h = win.default_height() as u32;
        rect_resize.set_size(w, h);
        label_resize.set_text(&format!("{}x{}", w, h));
    });

    // Initialize shared rect
    shared_rect.set_size(width as u32, height as u32);

    window.present();
    Ok(())
}
