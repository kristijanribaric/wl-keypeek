#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod connection;
mod device_discovery;
mod gtk_app;
mod key_matrix;
mod keyboard;
mod layout_key;
mod protocols;
mod qmk_keycode_labels;
mod rendering;
mod settings;
mod tray;
mod ui_wake;
mod zmk_keycode_labels;

use crate::connection::{build_connected_state, ConnectionRequest};
use crate::device_discovery::{discover_devices, DeviceKind, DiscoveredDevice};
use crate::gtk_app::GtkApp;
use crate::keyboard::Keyboard;
use crate::protocols::{ConnectionSpec, ZmkTransportConfig};
use crate::rendering::cairo_renderer::CairoRenderer;
use crate::settings::Settings;
use crate::tray::TrayEvent;
use crate::ui_wake::UiWake;
use glib::ControlFlow;
use gtk4::prelude::*;
use gtk4_layer_shell::LayerShell;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

enum UiEvent {
    KeyboardStateChanged,
    Connected(Keyboard),
    ConnectError(String),
}

fn main() {
    let settings = Settings::load().unwrap_or_default();
    let app = gtk4::Application::builder()
        .application_id("dev.srwi.keypeek")
        .build();

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>();
    let (tray_tx, tray_rx) = mpsc::channel::<TrayEvent>();

    if let Err(err) = tray::init_tray_service(tray_tx) {
        eprintln!("Failed to start tray service: {err}");
    }

    let settings = Rc::new(settings);
    let started = Rc::new(Cell::new(false));
    let ui_rx_slot = Rc::new(RefCell::new(Some(ui_rx)));
    let tray_rx_slot = Rc::new(RefCell::new(Some(tray_rx)));

    app.connect_activate(move |app| {
        if started.replace(true) {
            return;
        }

        let settings = settings.as_ref().clone();
        let gtk = GtkApp::new(app, &settings);
        let window = gtk.window.clone();
        let drawing_area = gtk.drawing_area.clone();

        let keyboard: Arc<Mutex<Option<Keyboard>>> = Arc::new(Mutex::new(None));
        let keyboard_for_draw = Arc::clone(&keyboard);
        let renderer = CairoRenderer::new(
            settings.size as f32,
            settings.font_size_multiplier,
            settings.theme.clone(),
        );

        drawing_area.set_draw_func(move |_, cr, width, height| {
            let keyboard_guard = keyboard_for_draw.lock().unwrap();
            if let Some(active_keyboard) = keyboard_guard.as_ref() {
                renderer.render_keyboard(cr, active_keyboard, width, height);
            }
        });

        let ui_wake = {
            let ui_tx = ui_tx.clone();
            UiWake::from_callback(move || {
                let _ = ui_tx.send(UiEvent::KeyboardStateChanged);
            })
        };

        let force_visible = Rc::new(Cell::new(false));

        let ui_rx = ui_rx_slot
            .borrow_mut()
            .take()
            .expect("UI event receiver already taken");
        let tray_rx = tray_rx_slot
            .borrow_mut()
            .take()
            .expect("Tray event receiver already taken");

        let ui_rx = Rc::new(RefCell::new(ui_rx));
        let tray_rx = Rc::new(RefCell::new(tray_rx));

        {
            let keyboard = Arc::clone(&keyboard);
            let drawing_area = drawing_area.clone();
            let window = window.clone();
            let force_visible = force_visible.clone();
            let settings = settings.clone();
            let app = app.clone();
            let ui_rx = ui_rx.clone();
            let tray_rx = tray_rx.clone();

            glib::timeout_add_local(Duration::from_millis(16), move || {
                process_ui_events(
                    &ui_rx.borrow(),
                    &window,
                    &keyboard,
                    &drawing_area,
                    &settings,
                    &force_visible,
                );

                if process_tray_events(&tray_rx.borrow(), &window, &force_visible, &app) {
                    update_overlay_visibility(&window, &keyboard, force_visible.get());
                    drawing_area.queue_draw();
                }

                update_overlay_visibility(&window, &keyboard, force_visible.get());
                ControlFlow::Continue
            });
        }

        start_connection_thread(settings.timeout, ui_wake, ui_tx.clone());
    });

    app.run();
}

fn process_ui_events(
    ui_rx: &Receiver<UiEvent>,
    window: &gtk4::ApplicationWindow,
    keyboard: &Arc<Mutex<Option<Keyboard>>>,
    drawing_area: &gtk4::DrawingArea,
    settings: &Settings,
    force_visible: &Cell<bool>,
) {
    loop {
        match ui_rx.try_recv() {
            Ok(event) => match event {
                UiEvent::KeyboardStateChanged => {
                    update_overlay_visibility(window, keyboard, force_visible.get());
                    drawing_area.queue_draw();
                }
                UiEvent::Connected(new_keyboard) => {
                    {
                        let mut keyboard_guard = keyboard.lock().unwrap();
                        *keyboard_guard = Some(new_keyboard);
                    }
                    resize_window_for_layout(window, keyboard, settings);
                    update_overlay_visibility(window, keyboard, force_visible.get());
                    drawing_area.queue_draw();
                }
                UiEvent::ConnectError(err) => {
                    eprintln!("Connection failed, retrying: {err}");
                }
            },
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn process_tray_events(
    tray_rx: &Receiver<TrayEvent>,
    window: &gtk4::ApplicationWindow,
    force_visible: &Cell<bool>,
    app: &gtk4::Application,
) -> bool {
    let mut changed_visibility = false;

    loop {
        match tray_rx.try_recv() {
            Ok(event) => match event {
                TrayEvent::ToggleVisibility => {
                    force_visible.set(!force_visible.get());
                    changed_visibility = true;
                }
                TrayEvent::AdjustX(delta) => {
                    let current = window.margin(gtk4_layer_shell::Edge::Left);
                    let new_x = (current + delta).max(0);
                    window.set_margin(gtk4_layer_shell::Edge::Left, new_x);
                }
                TrayEvent::AdjustY(delta) => {
                    let current = window.margin(gtk4_layer_shell::Edge::Top);
                    let new_y = (current + delta).max(0);
                    window.set_margin(gtk4_layer_shell::Edge::Top, new_y);
                }
                TrayEvent::ResetPosition => {
                    window.set_margin(gtk4_layer_shell::Edge::Left, 0);
                    window.set_margin(gtk4_layer_shell::Edge::Top, 0);
                }
                TrayEvent::Quit => app.quit(),
            },
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }

    changed_visibility
}

fn start_connection_thread(timeout: i64, ui_wake: UiWake, ui_tx: Sender<UiEvent>) {
    thread::Builder::new()
        .name("keypeek-zmk-connect".to_string())
        .spawn(move || {
            loop {
                let Some(spec) = find_zmk_connection_spec() else {
                    let _ = ui_tx.send(UiEvent::ConnectError(
                        "No ZMK device discovered; retrying".to_string(),
                    ));
                    thread::sleep(Duration::from_secs(3));
                    continue;
                };

                let request = ConnectionRequest {
                    spec,
                    timeout,
                    layout_name: None,
                };

                match build_connected_state(request, ui_wake.clone()) {
                    Ok(connected) => {
                        let _ = ui_tx.send(UiEvent::Connected(connected.keyboard));
                        break;
                    }
                    Err(err) => {
                        let _ = ui_tx.send(UiEvent::ConnectError(err));
                        thread::sleep(Duration::from_secs(3));
                    }
                }
            }
        })
        .expect("Failed to spawn ZMK connection thread");
}

fn find_zmk_connection_spec() -> Option<ConnectionSpec> {
    let devices = discover_devices();
    let selected = devices
        .iter()
        .filter(|device| device.kind == DeviceKind::Zmk)
        .max_by_key(zmk_device_priority)?;

    let transport = zmk_transport_for(selected)?;

    Some(ConnectionSpec::Zmk {
        vid: selected.vid,
        pid: selected.pid,
        transport,
    })
}

fn zmk_device_priority(device: &&DiscoveredDevice) -> u8 {
    if device.serial_port.is_some() {
        2
    } else if device.ble_device_id.is_some() {
        1
    } else {
        0
    }
}

fn zmk_transport_for(device: &DiscoveredDevice) -> Option<ZmkTransportConfig> {
    if let Some(port_name) = &device.serial_port {
        return Some(ZmkTransportConfig::Serial(port_name.clone()));
    }

    device
        .ble_device_id
        .as_ref()
        .map(|device_id| ZmkTransportConfig::Ble(device_id.clone()))
}

fn resize_window_for_layout(
    window: &gtk4::ApplicationWindow,
    keyboard: &Arc<Mutex<Option<Keyboard>>>,
    settings: &Settings,
) {
    let keyboard_guard = keyboard.lock().unwrap();
    let Some(active_keyboard) = keyboard_guard.as_ref() else {
        return;
    };

    let (layout_w, layout_h) = active_keyboard.layout.get_dimensions();
    let margin = settings.margin as i32;
    let width = (layout_w * settings.size as f32) as i32 + (margin * 2);
    let height = (layout_h * settings.size as f32) as i32 + (margin * 2);

    window.set_default_size(width.max(32), height.max(32));
}

fn update_overlay_visibility(
    window: &gtk4::ApplicationWindow,
    keyboard: &Arc<Mutex<Option<Keyboard>>>,
    force_visible: bool,
) {
    let should_show = if force_visible {
        true
    } else {
        let keyboard_guard = keyboard.lock().unwrap();
        if let Some(active_keyboard) = keyboard_guard.as_ref() {
            overlay_visible_for_keyboard(active_keyboard)
        } else {
            false
        }
    };

    if should_show {
        if !window.is_visible() {
            window.present();
        }
    } else if window.is_visible() {
        window.hide();
    }
}

fn overlay_visible_for_keyboard(keyboard: &Keyboard) -> bool {
    match keyboard.time_to_hide_overlay.lock().unwrap().as_ref() {
        Some(time_to_hide) => Instant::now() < *time_to_hide,
        None => true,
    }
}
