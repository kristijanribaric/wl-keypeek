//! GTK4 application with wlr-layer-shell support for Wayland compositors.
//! This module handles the main window, overlay layer, and drawing setup.

use crate::settings::{Settings, WindowPosition};
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, DrawingArea, EventControllerMotion, EventControllerScroll, EventControllerScrollFlags, GestureClick};
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::rc::Rc;

pub struct GtkApp {
    pub window: ApplicationWindow,
    pub drawing_area: DrawingArea,
}

#[derive(Default)]
struct DragState {
    dragging: bool,
    start_x: f64,
    start_y: f64,
    start_margin_x: i32,
    start_margin_y: i32,
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

        // Set up drag and scroll positioning
        let drag_state = Rc::new(RefCell::new(DragState::default()));
        setup_drag_reposition(&drawing_area, &window, drag_state);
        setup_scroll_positioning(&drawing_area, &window);

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

    // Always use Left and Top anchors for consistent positioning
    window.set_anchor(gtk4_layer_shell::Edge::Left, true);
    window.set_anchor(gtk4_layer_shell::Edge::Top, true);
    window.set_anchor(gtk4_layer_shell::Edge::Right, false);
    window.set_anchor(gtk4_layer_shell::Edge::Bottom, false);

    let margin = settings.margin as i32;
    window.set_margin(gtk4_layer_shell::Edge::Top, margin);
    window.set_margin(gtk4_layer_shell::Edge::Left, margin);
    window.set_margin(gtk4_layer_shell::Edge::Right, 0);
    window.set_margin(gtk4_layer_shell::Edge::Bottom, 0);

    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
}

fn setup_drag_reposition(drawing_area: &DrawingArea, window: &ApplicationWindow, drag_state: Rc<RefCell<DragState>>) {
    let gesture = GestureClick::new();
    let drag_state_press = drag_state.clone();
    let window_clone = window.clone();

    gesture.connect_pressed(move |_gesture, _n_press, x, y| {
        let mut state = drag_state_press.borrow_mut();
        state.dragging = true;
        state.start_x = x;
        state.start_y = y;
        state.start_margin_x = window_clone.margin(gtk4_layer_shell::Edge::Left);
        state.start_margin_y = window_clone.margin(gtk4_layer_shell::Edge::Top);
    });

    let drag_state_release = drag_state.clone();
    gesture.connect_released(move |_gesture, _n_press, _x, _y| {
        drag_state_release.borrow_mut().dragging = false;
    });

    drawing_area.add_controller(gesture);

    // Motion tracking for drag
    let motion_controller = EventControllerMotion::new();
    let drag_state_motion = drag_state;
    let window_motion = window.clone();

    motion_controller.connect_motion(move |_controller, x, y| {
        let mut state = drag_state_motion.borrow_mut();
        if state.dragging {
            let delta_x = (x - state.start_x) as i32;
            let delta_y = (y - state.start_y) as i32;

            let new_x = (state.start_margin_x + delta_x).max(0);
            let new_y = (state.start_margin_y + delta_y).max(0);

            window_motion.set_margin(gtk4_layer_shell::Edge::Left, new_x);
            window_motion.set_margin(gtk4_layer_shell::Edge::Top, new_y);
        }
    });

    drawing_area.add_controller(motion_controller);
}

fn setup_scroll_positioning(drawing_area: &DrawingArea, window: &ApplicationWindow) {
    let scroll_controller = EventControllerScroll::new(EventControllerScrollFlags::BOTH_AXES);
    let window_clone = window.clone();

    scroll_controller.connect_scroll(move |_controller, dx, dy| {
        let scroll_amount = 10i32;

        // Handle vertical scroll (dy) - move up/down
        if dy != 0.0 {
            let current_y = window_clone.margin(gtk4_layer_shell::Edge::Top);
            let new_y = if dy > 0.0 {
                (current_y + scroll_amount).max(0)
            } else {
                (current_y - scroll_amount).max(0)
            };

            window_clone.set_margin(gtk4_layer_shell::Edge::Top, new_y);
        }

        // Handle horizontal scroll (dx) - move left/right
        if dx != 0.0 {
            let current_x = window_clone.margin(gtk4_layer_shell::Edge::Left);
            let new_x = if dx > 0.0 {
                (current_x + scroll_amount).max(0)
            } else {
                (current_x - scroll_amount).max(0)
            };

            window_clone.set_margin(gtk4_layer_shell::Edge::Left, new_x);
        }

        gdk::glib::signal::Propagation::Stop
    });

    drawing_area.add_controller(scroll_controller);
}















