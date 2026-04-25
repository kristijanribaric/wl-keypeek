use image::GenericImageView;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use std::sync::LazyLock;
use std::sync::mpsc::Sender;

#[derive(Clone, Debug)]
pub enum TrayEvent {
    ToggleVisibility,
    AdjustX(i32),  // adjust by this delta
    AdjustY(i32),  // adjust by this delta
    AdjustScale(i32),
    ToggleEnabled,
    ResetPosition,
    Quit,
}

struct KeyPeekTray {
    sender: Sender<TrayEvent>,
    force_visible: bool,
    overlay_enabled: bool,
}

static TRAY_ICON: LazyLock<Option<ksni::Icon>> = LazyLock::new(|| {
    let png = include_bytes!("../resources/icon.png");
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|err| {
            eprintln!("Failed to decode tray icon PNG: {err}");
            err
        })
        .ok()?;

    let (width, height) = image.dimensions();
    let mut data = image.into_rgba8().into_vec();
    for pixel in data.chunks_exact_mut(4) {
        // StatusNotifier expects ARGB, while decoded PNG pixels are RGBA.
        pixel.rotate_right(1);
    }

    Some(ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    })
});

impl ksni::Tray for KeyPeekTray {
    fn id(&self) -> String {
        "keypeek-wayland".into()
    }

    fn title(&self) -> String {
        "KeyPeek".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        TRAY_ICON.iter().cloned().collect()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let toggle_label = if self.force_visible {
            "Hide"
        } else {
            "Show"
        };
        let enabled_label = if self.overlay_enabled {
            "Disable Overlay"
        } else {
            "Enable Overlay"
        };

        vec![
            StandardItem {
                label: toggle_label.into(),
                activate: Box::new(|tray: &mut Self| {
                    if tray.overlay_enabled {
                        tray.force_visible = !tray.force_visible;
                        let _ = tray.sender.send(TrayEvent::ToggleVisibility);
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: enabled_label.into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.overlay_enabled = !tray.overlay_enabled;
                    if !tray.overlay_enabled {
                        tray.force_visible = false;
                    }
                    let _ = tray.sender.send(TrayEvent::ToggleEnabled);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Scale -".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustScale(-5));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Scale +".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustScale(5));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Position ← (X-10)".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustX(-10));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Position → (X+10)".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustX(10));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Position ↑ (Y-10)".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustY(-10));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Position ↓ (Y+10)".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::AdjustY(10));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Reset Position".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::ResetPosition);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn init_tray_service(
    sender: Sender<TrayEvent>,
    overlay_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    std::thread::Builder::new()
        .name("keypeek-tray".to_string())
        .spawn(move || {
            let tray = KeyPeekTray {
                sender,
                force_visible: false,
                overlay_enabled,
            };

            match tray.spawn() {
                Ok(_handle) => loop {
                    std::thread::park();
                },
                Err(err) => {
                    eprintln!("Failed to register tray item: {err}");
                }
            }
        })?;

    Ok(())
}
