//! GTK4 application with wlr-layer-shell support for Wayland compositors.
//! This module handles the main window, overlay layer, and drawing setup.

use crate::settings::{Settings, WindowPosition};
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, DrawingArea};
use gtk4_layer_shell::LayerShell;

pub struct GtkApp {
    pub window: ApplicationWindow,
    pub drawing_area: DrawingArea,
}

impl GtkApp {
    /// Create and initialize the GTK4 overlay widgets for an active application.
    pub fn new(app: &Application, settings: &Settings) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("KeyPeek")
            .decorated(false)
            .focusable(false)
            .build();
        window.set_can_focus(false);
        window.set_resizable(false);

        let drawing_area = DrawingArea::new();
        drawing_area.set_hexpand(true);
        drawing_area.set_vexpand(true);
        window.set_child(Some(&drawing_area));

        apply_transparent_css();
        configure_layer_shell(&window, settings);

        // Hidden by default; shown only while non-base layers are active.
        window.set_visible(false);

        Self {
            window,
            drawing_area,
        }
    }
}

fn apply_transparent_css() {
    let css = CssProvider::new();
    css.load_from_data(
        "window, box, drawingarea { background-color: transparent; border: none; box-shadow: none; }",
    );

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn configure_layer_shell(window: &ApplicationWindow, settings: &Settings) {
    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);

    for edge in [
        gtk4_layer_shell::Edge::Left,
        gtk4_layer_shell::Edge::Right,
        gtk4_layer_shell::Edge::Top,
        gtk4_layer_shell::Edge::Bottom,
    ] {
        window.set_anchor(edge, false);
        window.set_margin(edge, 0);
    }

    let margin = settings.margin as i32;
    match settings.position {
        WindowPosition::TopLeft => {
            window.set_anchor(gtk4_layer_shell::Edge::Top, true);
            window.set_anchor(gtk4_layer_shell::Edge::Left, true);
            window.set_margin(gtk4_layer_shell::Edge::Top, margin);
            window.set_margin(gtk4_layer_shell::Edge::Left, margin);
        }
        WindowPosition::TopRight => {
            window.set_anchor(gtk4_layer_shell::Edge::Top, true);
            window.set_anchor(gtk4_layer_shell::Edge::Right, true);
            window.set_margin(gtk4_layer_shell::Edge::Top, margin);
            window.set_margin(gtk4_layer_shell::Edge::Right, margin);
        }
        WindowPosition::BottomLeft => {
            window.set_anchor(gtk4_layer_shell::Edge::Bottom, true);
            window.set_anchor(gtk4_layer_shell::Edge::Left, true);
            window.set_margin(gtk4_layer_shell::Edge::Bottom, margin);
            window.set_margin(gtk4_layer_shell::Edge::Left, margin);
        }
        WindowPosition::BottomRight => {
            window.set_anchor(gtk4_layer_shell::Edge::Bottom, true);
            window.set_anchor(gtk4_layer_shell::Edge::Right, true);
            window.set_margin(gtk4_layer_shell::Edge::Bottom, margin);
            window.set_margin(gtk4_layer_shell::Edge::Right, margin);
        }
        WindowPosition::Top => {
            window.set_anchor(gtk4_layer_shell::Edge::Top, true);
            window.set_margin(gtk4_layer_shell::Edge::Top, margin);
        }
        WindowPosition::Bottom => {
            window.set_anchor(gtk4_layer_shell::Edge::Bottom, true);
            window.set_margin(gtk4_layer_shell::Edge::Bottom, margin);
        }
    }

    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
}
