//! Cairo-based keyboard layout renderer.
//! Ports the egui rendering logic to Cairo for GTK4.

use crate::keyboard::Keyboard;
use crate::layout_key::{KeycodeKind, LayoutKey};
use crate::settings::ThemeColor;
use cairo::Context;
use pango::{Alignment, EllipsizeMode, Layout, WrapMode};
use std::f64::consts::PI;

pub struct CairoRenderer {
    pub size: f32,
    pub font_scale: f32,
    pub theme: crate::settings::ThemeSettings,
}

impl CairoRenderer {
    const PANGO_SCALE: i32 = 1024;
    const FONT_FAMILY_FALLBACK: &'static str = "Inter, Roboto, Cantarell, Noto Sans, Sans";

    pub fn new(size: f32, font_scale: f32, theme: crate::settings::ThemeSettings) -> Self {
        Self {
            size,
            font_scale,
            theme,
        }
    }

    /// Render the entire keyboard layout.
    pub fn render_keyboard(&self, cr: &Context, keyboard: &Keyboard, width: i32, height: i32) {
        // Clear background (transparent)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint().unwrap();

        let size = self.size as f64;
        let font_scale = self.font_scale as f64;

        // Get layout dimensions
        let (layout_width, layout_height) = keyboard.layout.get_dimensions();
        let layout_width = (layout_width as f64) * size;
        let layout_height = (layout_height as f64) * size;

        // Center the layout in the window
        let offset_x = ((width as f64 - layout_width) / 2.0).max(0.0);
        let offset_y = ((height as f64 - layout_height) / 2.0).max(0.0);

        // Draw each key
        for key in &keyboard.layout.keys {
            let (effective_layer, is_background_key) =
                keyboard.get_effective_key_layer(key.row, key.col);

            let layout_key = keyboard
                .get_key(effective_layer as usize, key.row, key.col)
                .unwrap_or_default();

            let first_layer_key_kind = keyboard
                .get_key(0, key.row, key.col)
                .map(|k| k.kind)
                .unwrap_or(KeycodeKind::Basic);

            let (fill_color, stroke_color, border_thickness, font_color) = self.get_keycode_color(
                layout_key.layer_ref.unwrap_or(effective_layer),
                first_layer_key_kind,
                is_background_key,
                keyboard.is_key_pressed(key.row, key.col),
            );

            let key_x = offset_x + (key.x as f64) * size;
            let key_y = offset_y + (key.y as f64) * size;
            let key_w = (key.w as f64) * size;
            let key_h = (key.h as f64) * size;

            // Draw rounded rectangle with border
            self.draw_key_background(cr, key_x, key_y, key_w, key_h, fill_color, stroke_color, border_thickness);

            // Draw text
            self.draw_key_label(
                cr,
                &layout_key,
                key_x,
                key_y,
                key_w,
                key_h,
                font_color,
                0.25 * size * font_scale,
            );
        }
    }

    /// Draw a single key background (rounded rectangle with border).
    fn draw_key_background(
        &self,
        cr: &Context,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: (u8, u8, u8, u8),
        stroke: (u8, u8, u8, u8),
        border_width: f64,
    ) {
        let radius = 0.1 * self.size as f64;
        let shrink = 0.06 * self.size as f64;

        let x = x + shrink;
        let y = y + shrink;
        let w = w - 2.0 * shrink;
        let h = h - 2.0 * shrink;

        // Draw rounded rectangle
        cr.new_path();
        cr.arc(x + radius, y + radius, radius, PI, PI * 1.5);
        cr.arc(x + w - radius, y + radius, radius, PI * 1.5, PI * 2.0);
        cr.arc(x + w - radius, y + h - radius, radius, 0.0, PI * 0.5);
        cr.arc(x + radius, y + h - radius, radius, PI * 0.5, PI);
        cr.close_path();

        // Fill
        cr.set_source_rgba(
            fill.0 as f64 / 255.0,
            fill.1 as f64 / 255.0,
            fill.2 as f64 / 255.0,
            fill.3 as f64 / 255.0,
        );
        cr.fill_preserve().unwrap();

        // Stroke
        cr.set_line_width(border_width);
        cr.set_source_rgba(
            stroke.0 as f64 / 255.0,
            stroke.1 as f64 / 255.0,
            stroke.2 as f64 / 255.0,
            stroke.3 as f64 / 255.0,
        );
        cr.stroke().unwrap();
    }

    /// Draw the key label (text and/or symbol).
    fn draw_key_label(
        &self,
        cr: &Context,
        key: &LayoutKey,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        font_color: (u8, u8, u8, u8),
        font_size: f64,
    ) {
        cr.set_source_rgba(
            font_color.0 as f64 / 255.0,
            font_color.1 as f64 / 255.0,
            font_color.2 as f64 / 255.0,
            font_color.3 as f64 / 255.0,
        );

        // Get appropriate text to display
        let text = if !key.tap.full.is_empty() {
            key.tap.full.clone()
        } else if let Some(short) = &key.tap.short {
            short.clone()
        } else {
            return; // Nothing to display
        };

        // Create a Pango layout for text measurement and rendering
        let pango_context = pangocairo::functions::create_context(cr);
        let layout = Layout::new(&pango_context);

        layout.set_text(&text);
        layout.set_alignment(Alignment::Center);
        let max_width = (w - 0.28 * self.size as f64).max(1.0);
        let max_height = (h - 0.28 * self.size as f64).max(1.0);

        // Prefer a clean sans-serif stack and dynamically downscale to fit before truncating.
        let (text_width, text_height) =
            self.fit_layout_to_key(&layout, font_size, max_width, max_height, &text);

        // Center text in key
        let text_x = x + (w - text_width) / 2.0;
        let text_y = y + (h - text_height) / 2.0;

        cr.move_to(text_x, text_y);
        pangocairo::functions::show_layout(cr, &layout);
    }

    fn fit_layout_to_key(
        &self,
        layout: &Layout,
        base_font_size: f64,
        max_width: f64,
        max_height: f64,
        text: &str,
    ) -> (f64, f64) {
        let min_font_size = (base_font_size * 0.55).max(7.0);
        let mut current_size = base_font_size;

        while current_size >= min_font_size {
            self.configure_layout(layout, text, current_size, max_width, false, None);
            let (w, h) = self.layout_size(layout);
            if w <= max_width && h <= max_height {
                return (w, h);
            }
            current_size -= 0.5;
        }

        // Fallback to wrapping over two lines if one-line shrinking still does not fit.
        self.configure_layout(
            layout,
            text,
            min_font_size,
            max_width,
            true,
            Some(EllipsizeMode::None),
        );
        let (wrapped_w, wrapped_h) = self.layout_size(layout);
        if wrapped_w <= max_width && wrapped_h <= max_height {
            return (wrapped_w, wrapped_h);
        }

        // Last resort: preserve readability with ellipsis rather than clipping.
        self.configure_layout(
            layout,
            text,
            min_font_size,
            max_width,
            true,
            Some(EllipsizeMode::End),
        );
        self.layout_size(layout)
    }

    fn configure_layout(
        &self,
        layout: &Layout,
        text: &str,
        font_size: f64,
        max_width: f64,
        wrapped: bool,
        ellipsize: Option<EllipsizeMode>,
    ) {
        let mut font_desc = pango::FontDescription::new();
        font_desc.set_family(Self::FONT_FAMILY_FALLBACK);
        font_desc.set_size((font_size * Self::PANGO_SCALE as f64) as i32);
        layout.set_font_description(Some(&font_desc));
        layout.set_text(text);
        layout.set_width((max_width.max(1.0) as i32) * Self::PANGO_SCALE);

        if wrapped {
            layout.set_wrap(WrapMode::WordChar);
            layout.set_height(2 * Self::PANGO_SCALE);
            layout.set_width((max_width.max(1.0) as i32) * Self::PANGO_SCALE);
        } else {
            layout.set_wrap(WrapMode::Word);
            layout.set_height(-1);
            layout.set_width(-1);
        }

        layout.set_ellipsize(ellipsize.unwrap_or(EllipsizeMode::None));
    }

    fn layout_size(&self, layout: &Layout) -> (f64, f64) {
        let (_ink_rect, logical_rect) = layout.extents();
        (
            logical_rect.width() as f64 / Self::PANGO_SCALE as f64,
            logical_rect.height() as f64 / Self::PANGO_SCALE as f64,
        )
    }

    /// Calculate key color based on layer, kind, and state.
    /// Returns (fill_rgb, stroke_rgb, border_width, text_rgb) as RGBA tuples.
    fn get_keycode_color(
        &self,
        layer: u8,
        kind: KeycodeKind,
        desaturate: bool,
        pressed: bool,
    ) -> ((u8, u8, u8, u8), (u8, u8, u8, u8), f64, (u8, u8, u8, u8)) {
        const DESATURATE_FACTOR: f32 = 0.7;

        let size = self.size as f64;
        let layer_theme_color = self.theme.layer_color(layer);
        let mut background_color = layer_theme_color;
        let mut font_color = self.theme.font_color;

        if pressed {
            // Pressed state: lighter background
            background_color = self.lerp_color(background_color, ThemeColor::new(255, 255, 255, 255), 0.2);
            let stroke_color = self.lerp_color(background_color, ThemeColor::new(255, 255, 255, 255), 0.7);
            let font_color = self.lerp_color(font_color, ThemeColor::new(255, 255, 255, 255), 0.4);
            return (
                self.color_to_rgba(background_color),
                self.color_to_rgba(stroke_color),
                0.03 * size,
                self.color_to_rgba(font_color),
            );
        }

        // Apply kind-specific darkening
        if kind == KeycodeKind::Special {
            background_color = self.lerp_color(background_color, ThemeColor::new(0, 0, 0, 255), 0.6);
        } else if kind == KeycodeKind::Modifier {
            background_color = self.lerp_color(background_color, ThemeColor::new(0, 0, 0, 255), 0.3);
        }

        let mut border_color = self.lerp_color(background_color, ThemeColor::new(0, 0, 0, 255), 0.2);

        // Apply desaturation for background keys
        if desaturate && layer != 0 {
            let layer0_color = self.theme.layer_colors[0];
            background_color = self.lerp_color(background_color, layer0_color, DESATURATE_FACTOR);
            border_color = self.lerp_color(border_color, layer0_color, DESATURATE_FACTOR);
            font_color = ThemeColor::new(
                (font_color.r as f32 * (1.0 - DESATURATE_FACTOR)) as u8,
                (font_color.g as f32 * (1.0 - DESATURATE_FACTOR)) as u8,
                (font_color.b as f32 * (1.0 - DESATURATE_FACTOR)) as u8,
                font_color.a,
            );
        }

        (
            self.color_to_rgba(background_color),
            self.color_to_rgba(border_color),
            1.0,
            self.color_to_rgba(font_color),
        )
    }

    /// Linear interpolation between two colors.
    fn lerp_color(&self, a: ThemeColor, b: ThemeColor, t: f32) -> ThemeColor {
        ThemeColor::new(
            (a.r as f32 + (b.r as f32 - a.r as f32) * t) as u8,
            (a.g as f32 + (b.g as f32 - a.g as f32) * t) as u8,
            (a.b as f32 + (b.b as f32 - a.b as f32) * t) as u8,
            (a.a as f32 + (b.a as f32 - a.a as f32) * t) as u8,
        )
    }

    /// Convert ThemeColor to RGBA tuple for cairo.
    fn color_to_rgba(&self, color: ThemeColor) -> (u8, u8, u8, u8) {
        (color.r, color.g, color.b, color.a)
    }
}





