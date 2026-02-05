//! Display-oriented key representation.
//!
//! `LayoutKey` is the unified abstraction for representing a key's display labels,
//! independent of the source firmware (QMK, ZMK, etc.). It provides all the information
//! needed to render a key's label in the overlay.
//!
//! # Transparency
//! Transparent keys are represented as `None` when stored in collections like
//! `Vec<Vec<Vec<Option<LayoutKey>>>>`. This makes layer fall-through logic simple:
//! just check `key.is_some()`.

use crate::keycode_labels::get_layout_key;
use qmk_via_api::keycodes::Keycode;

/// Visual classification for key coloring.
#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum KeycodeKind {
    #[default]
    Basic,
    Modifier,
    Special,
}

/// A text label with optional short variant.
///
/// Used for both tap and hold labels in `LayoutKey`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Label {
    /// Full label text (e.g., "Enter", "Shift")
    pub full: String,

    /// Optional shorter version (e.g., "Ent", "Shft")
    pub short: Option<String>,
}

impl Label {
    /// Create a label with just the full text.
    pub fn new(full: impl Into<String>) -> Self {
        Label {
            full: full.into(),
            short: None,
        }
    }

    /// Create a label with both full and short text.
    pub fn with_short(full: impl Into<String>, short: impl Into<String>) -> Self {
        Label {
            full: full.into(),
            short: Some(short.into()),
        }
    }

    /// Check if the label is empty.
    pub fn is_empty(&self) -> bool {
        self.full.is_empty()
    }
}

impl From<&str> for Label {
    fn from(s: &str) -> Self {
        Label::new(s)
    }
}

impl From<String> for Label {
    fn from(s: String) -> Self {
        Label::new(s)
    }
}

/// A key's display representation, containing all label variants and metadata.
///
/// This struct is firmware-agnostic: both QMK keycodes and ZMK bindings
/// are converted into this unified format for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutKey {
    /// Primary key action label (e.g., "A", "Enter", "L1")
    pub tap: Label,

    /// Hold action label for hold-tap keys (e.g., "Shift" for MT(LSFT, KC_A))
    pub hold: Option<Label>,

    /// Symbol/icon for the key (using Phosphor icon font)
    pub symbol: Option<String>,

    /// Visual classification for coloring
    pub kind: KeycodeKind,

    /// Layer this key activates (for MO, LT, TO, etc.) - used for coloring
    pub layer_ref: Option<u8>,
}

impl Default for LayoutKey {
    fn default() -> Self {
        LayoutKey {
            tap: Label::default(),
            hold: None,
            symbol: None,
            kind: KeycodeKind::Basic,
            layer_ref: None,
        }
    }
}

impl LayoutKey {
    /// Create a LayoutKey from a QMK keycode.
    ///
    /// Returns `None` for `KC_TRANSPARENT` (0x0001), which should be represented
    /// as `None` in the key matrix to enable proper layer fall-through.
    pub fn from_qmk_keycode(keycode: u16) -> Option<Self> {
        // KC_TRANSPARENT is special: return None to represent transparency
        if keycode == Keycode::KC_TRANSPARENT as u16 {
            return None;
        }

        get_layout_key(keycode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_qmk_keycode_basic() {
        let key = LayoutKey::from_qmk_keycode(Keycode::KC_A as u16);
        assert!(key.is_some());
        let key = key.unwrap();
        assert_eq!(key.tap.full, "A");
        assert_eq!(key.kind, KeycodeKind::Basic);
    }

    #[test]
    fn test_from_qmk_keycode_transparent() {
        let key = LayoutKey::from_qmk_keycode(Keycode::KC_TRANSPARENT as u16);
        assert!(key.is_none());
    }

    #[test]
    fn test_from_qmk_keycode_no() {
        let key = LayoutKey::from_qmk_keycode(Keycode::KC_NO as u16);
        assert!(key.is_some());
        // KC_NO should produce an empty tap label
        let key = key.unwrap();
        assert_eq!(key.tap.full, "");
    }
}
