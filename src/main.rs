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

    if let Err(err) = tray::init_tray_service(
        tray_tx,
        settings.overlay_enabled,
        settings.delay_close_on_default_layer,
    ) {
        eprintln!("Failed to start tray service: {err}");
    }

    let settings = Rc::new(RefCell::new(settings));
    let started = Rc::new(Cell::new(false));
    let ui_rx_slot = Rc::new(RefCell::new(Some(ui_rx)));
    let tray_rx_slot = Rc::new(RefCell::new(Some(tray_rx)));

    app.connect_activate(move |app| {
        if started.replace(true) {
            return;
        }

        let settings_snapshot = {
            let s = settings.borrow();
            s.clone()
        };

        let gtk = GtkApp::new(app, &settings_snapshot);
        let window = gtk.window.clone();
        let drawing_area = gtk.drawing_area.clone();

        // Apply saved position offsets
        // On first run, offset_x/offset_y are 0 (default), so initialize from margin
        let initial_x = if settings_snapshot.offset_x > 0 {
            settings_snapshot.offset_x
        } else {
            settings_snapshot.margin as i32
        };
        let initial_y = if settings_snapshot.offset_y > 0 {
            settings_snapshot.offset_y
        } else {
            settings_snapshot.margin as i32
        };
        window.set_margin(gtk4_layer_shell::Edge::Left, initial_x);
        window.set_margin(gtk4_layer_shell::Edge::Top, initial_y);

        // Update settings to reflect initial position
        {
            let mut s = settings.borrow_mut();
            s.offset_x = initial_x;
            s.offset_y = initial_y;
        }

        let keyboard: Arc<Mutex<Option<Keyboard>>> = Arc::new(Mutex::new(None));
        let keyboard_for_draw = Arc::clone(&keyboard);
        let settings_for_draw = settings.clone();

        drawing_area.set_draw_func(move |_, cr, width, height| {
            let keyboard_guard = keyboard_for_draw.lock().unwrap();
            if let Some(active_keyboard) = keyboard_guard.as_ref() {
                let settings_ref = settings_for_draw.borrow();
                let renderer = CairoRenderer::new(
                    settings_ref.size as f32,
                    settings_ref.font_size_multiplier,
                    settings_ref.theme.clone(),
                );
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

        // Track position to detect drag/scroll changes
        let last_pos = Rc::new(RefCell::new((initial_x, initial_y)));

        {
            let keyboard = Arc::clone(&keyboard);
            let drawing_area = drawing_area.clone();
            let window = window.clone();
            let force_visible = force_visible.clone();
            let settings = settings.clone();
            let app = app.clone();
            let ui_rx = ui_rx.clone();
            let tray_rx = tray_rx.clone();
            let last_pos = last_pos.clone();

            glib::timeout_add_local(Duration::from_millis(16), move || {
                let settings_ref = settings.borrow();
                process_ui_events(
                    &ui_rx.borrow(),
                    &window,
                    &keyboard,
                    &drawing_area,
                    &settings_ref,
                    &force_visible,
                );
                drop(settings_ref); // Release borrow

                if process_tray_events(
                    &tray_rx.borrow(),
                    &window,
                    &keyboard,
                    &force_visible,
                    &drawing_area,
                    &app,
                    &settings,
                ) {
                    let overlay_enabled = settings.borrow().overlay_enabled;
                    update_overlay_visibility(
                        &window,
                        &keyboard,
                        force_visible.get(),
                        overlay_enabled,
                    );
                    drawing_area.queue_draw();
                }

                // Check if position changed (from drag/scroll) and persist it
                let current_x = window.margin(gtk4_layer_shell::Edge::Left);
                let current_y = window.margin(gtk4_layer_shell::Edge::Top);
                let mut last = last_pos.borrow_mut();
                if last.0 != current_x || last.1 != current_y {
                    last.0 = current_x;
                    last.1 = current_y;
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.offset_x = current_x;
                    settings_ref.offset_y = current_y;
                    let _ = settings_ref.save();
                }

                let overlay_enabled = settings.borrow().overlay_enabled;
                update_overlay_visibility(&window, &keyboard, force_visible.get(), overlay_enabled);
                ControlFlow::Continue
            });
        }

        start_connection_thread(
            settings.borrow().timeout,
            settings.borrow().delay_close_on_default_layer,
            ui_wake,
            ui_tx.clone(),
        );
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
                    update_overlay_visibility(
                        window,
                        keyboard,
                        force_visible.get(),
                        settings.overlay_enabled,
                    );
                    drawing_area.queue_draw();
                }
                UiEvent::Connected(new_keyboard) => {
                    new_keyboard.set_timeout(settings.timeout);
                    new_keyboard
                        .set_delay_close_on_default_layer(settings.delay_close_on_default_layer);
                    {
                        let mut keyboard_guard = keyboard.lock().unwrap();
                        *keyboard_guard = Some(new_keyboard);
                    }
                    resize_window_for_layout(window, keyboard, settings);
                    update_overlay_visibility(
                        window,
                        keyboard,
                        force_visible.get(),
                        settings.overlay_enabled,
                    );
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
    keyboard: &Arc<Mutex<Option<Keyboard>>>,
    force_visible: &Cell<bool>,
    drawing_area: &gtk4::DrawingArea,
    app: &gtk4::Application,
    settings: &Rc<RefCell<Settings>>,
) -> bool {
    let mut needs_refresh = false;

    loop {
        match tray_rx.try_recv() {
            Ok(event) => match event {
                TrayEvent::ToggleVisibility => {
                    if settings.borrow().overlay_enabled {
                        force_visible.set(!force_visible.get());
                        needs_refresh = true;
                    }
                }
                TrayEvent::ToggleEnabled => {
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.overlay_enabled = !settings_ref.overlay_enabled;
                    if !settings_ref.overlay_enabled {
                        force_visible.set(false);
                    }
                    let _ = settings_ref.save();
                    needs_refresh = true;
                }
                TrayEvent::ToggleDelayedClose => {
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.delay_close_on_default_layer =
                        !settings_ref.delay_close_on_default_layer;
                    if let Some(active_keyboard) = keyboard.lock().unwrap().as_ref() {
                        active_keyboard.set_delay_close_on_default_layer(
                            settings_ref.delay_close_on_default_layer,
                        );
                    }
                    let _ = settings_ref.save();
                    needs_refresh = true;
                }
                TrayEvent::AdjustScale(delta) => {
                    let mut settings_ref = settings.borrow_mut();
                    let new_size = clamp_overlay_size(settings_ref.size + delta);
                    if new_size != settings_ref.size {
                        settings_ref.size = new_size;
                        let _ = settings_ref.save();
                        resize_window_for_layout(window, keyboard, &settings_ref);
                        drawing_area.queue_draw();
                        needs_refresh = true;
                    }
                }
                TrayEvent::AdjustX(delta) => {
                    let current = window.margin(gtk4_layer_shell::Edge::Left);
                    let new_x = (current + delta).max(0);
                    window.set_margin(gtk4_layer_shell::Edge::Left, new_x);
                    // Update and persist settings
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.offset_x = new_x;
                    let _ = settings_ref.save();
                }
                TrayEvent::AdjustY(delta) => {
                    let current = window.margin(gtk4_layer_shell::Edge::Top);
                    let new_y = (current + delta).max(0);
                    window.set_margin(gtk4_layer_shell::Edge::Top, new_y);
                    // Update and persist settings
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.offset_y = new_y;
                    let _ = settings_ref.save();
                }
                TrayEvent::ResetPosition => {
                    window.set_margin(gtk4_layer_shell::Edge::Left, 0);
                    window.set_margin(gtk4_layer_shell::Edge::Top, 0);
                    // Update and persist settings
                    let mut settings_ref = settings.borrow_mut();
                    settings_ref.offset_x = 0;
                    settings_ref.offset_y = 0;
                    let _ = settings_ref.save();
                }
                TrayEvent::Quit => app.quit(),
            },
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }

    needs_refresh
}

fn start_connection_thread(
    timeout: i64,
    delay_close_on_default_layer: bool,
    ui_wake: UiWake,
    ui_tx: Sender<UiEvent>,
) {
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
                    delay_close_on_default_layer,
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

fn clamp_overlay_size(size: i32) -> i32 {
    size.clamp(20, 1000)
}

fn update_overlay_visibility(
    window: &gtk4::ApplicationWindow,
    keyboard: &Arc<Mutex<Option<Keyboard>>>,
    force_visible: bool,
    overlay_enabled: bool,
) {
    let should_show = if !overlay_enabled {
        false
    } else if force_visible {
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
