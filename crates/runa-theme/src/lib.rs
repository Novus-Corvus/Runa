//! # runa-theme
//!
//! Colours for Runa, with no UI toolkit attached.
//!
//! Pick a [`Mode`] (dark or light) and an accent colour, and [`Palette::new`]
//! derives every colour the interface needs. The accent can change at any time
//! by building a new palette, which is what makes "change the accent live in
//! Appearance settings" a one-line operation for the UI.
//!
//! Accessibility is computed, not hoped for: whatever accent the person picks,
//! text drawn in the accent colour is nudged until it reaches a WCAG contrast
//! ratio of at least 4.5 against the page backgrounds, and the text drawn *on*
//! an accent-coloured button is automatically black or white, whichever reads
//! better.
//!
//! The names mirror the concepts of the Obsidian interface (primary and
//! secondary backgrounds, border, normal/muted/faint text, interactive accent)
//! so a UI can map one to the other directly.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::fmt;

/// Minimum contrast ratio for readable text (WCAG AA, normal size).
pub const MIN_TEXT_CONTRAST: f64 = 4.5;

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red, 0-255.
    pub r: u8,
    /// Green, 0-255.
    pub g: u8,
    /// Blue, 0-255.
    pub b: u8,
}

/// A colour in hue/saturation/lightness form. `h` is in degrees (0-360),
/// `s` and `l` are 0-1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    /// Hue in degrees.
    pub h: f64,
    /// Saturation, 0 to 1.
    pub s: f64,
    /// Lightness, 0 to 1.
    pub l: f64,
}

/// Why a colour could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The text is not `#RGB` or `#RRGGBB`.
    InvalidHex(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHex(text) => write!(
                f,
                "\"{text}\" is not a colour. Use a hex code such as #C8372D or #C33."
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

impl Rgb {
    /// Builds a colour from its channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#RRGGBB` or `#RGB` (the `#` is optional, case-insensitive).
    pub fn from_hex(text: &str) -> Result<Self, ThemeError> {
        let trimmed = text.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        let values: Option<Vec<u8>> = digits
            .bytes()
            .map(|byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            })
            .collect();
        match values.as_deref() {
            Some([r1, r2, g1, g2, b1, b2]) => {
                Ok(Self::new(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2))
            }
            Some([r, g, b]) => Ok(Self::new(r * 17, g * 17, b * 17)),
            _ => Err(ThemeError::InvalidHex(text.to_string())),
        }
    }

    /// Formats as `#RRGGBB`.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// WCAG relative luminance, 0 (black) to 1 (white).
    pub fn luminance(self) -> f64 {
        fn linear(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio against `other`, from 1 (identical) to 21.
    pub fn contrast(self, other: Self) -> f64 {
        let a = self.luminance();
        let b = other.luminance();
        let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Linear blend: `t = 0` is `self`, `t = 1` is `other`.
    pub fn mix(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let blend = |a: u8, b: u8| -> u8 {
            let value = f64::from(a) + (f64::from(b) - f64::from(a)) * t;
            value.round().clamp(0.0, 255.0) as u8
        };
        Self::new(
            blend(self.r, other.r),
            blend(self.g, other.g),
            blend(self.b, other.b),
        )
    }

    /// Converts to hue/saturation/lightness.
    pub fn to_hsl(self) -> Hsl {
        let r = f64::from(self.r) / 255.0;
        let g = f64::from(self.g) / 255.0;
        let b = f64::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let delta = max - min;
        if delta.abs() < f64::EPSILON {
            return Hsl { h: 0.0, s: 0.0, l };
        }
        let s = delta / (1.0 - (2.0 * l - 1.0).abs());
        let h = if (max - r).abs() < f64::EPSILON {
            ((g - b) / delta).rem_euclid(6.0)
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        } * 60.0;
        Hsl { h, s, l }
    }

    /// Converts from hue/saturation/lightness (inputs are clamped to range).
    pub fn from_hsl(hsl: Hsl) -> Self {
        let h = hsl.h.rem_euclid(360.0);
        let s = hsl.s.clamp(0.0, 1.0);
        let l = hsl.l.clamp(0.0, 1.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = l - c / 2.0;
        let (r, g, b) = match (h / 60.0) as u8 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let channel = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Self::new(channel(r), channel(g), channel(b))
    }

    /// Same colour with `delta` added to its lightness (clamped).
    pub fn shift_lightness(self, delta: f64) -> Self {
        let hsl = self.to_hsl();
        Self::from_hsl(Hsl {
            l: (hsl.l + delta).clamp(0.0, 1.0),
            ..hsl
        })
    }
}

/// Dark or light interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Dark surfaces, light text.
    Dark,
    /// Light surfaces, dark text.
    Light,
}

/// A named accent colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPreset {
    /// Display name.
    pub name: &'static str,
    /// The colour.
    pub color: Rgb,
}

/// The default accent: a deep rune-carving red.
pub const RUNE_RED: Rgb = Rgb::new(0xC8, 0x37, 0x2D);

/// Built-in accents, default first. Any custom colour also works.
pub const ACCENT_PRESETS: [AccentPreset; 6] = [
    AccentPreset {
        name: "Rune Red",
        color: RUNE_RED,
    },
    AccentPreset {
        name: "Ember",
        color: Rgb::new(0xE0, 0x7A, 0x2F),
    },
    AccentPreset {
        name: "Gold",
        color: Rgb::new(0xD4, 0xA7, 0x2C),
    },
    AccentPreset {
        name: "Yggdrasil Green",
        color: Rgb::new(0x4E, 0x9F, 0x5D),
    },
    AccentPreset {
        name: "Mimir Blue",
        color: Rgb::new(0x3E, 0x7C, 0xC4),
    },
    AccentPreset {
        name: "Seidr Violet",
        color: Rgb::new(0x8B, 0x5C, 0xC8),
    },
];

/// Every colour the interface needs, for one mode and accent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Dark or light.
    pub mode: Mode,
    /// Main editor and content background.
    pub bg_primary: Rgb,
    /// Sidebars, ribbons and panels.
    pub bg_secondary: Rgb,
    /// Dividers and outlines.
    pub border: Rgb,
    /// Body text.
    pub text_normal: Rgb,
    /// Secondary text.
    pub text_muted: Rgb,
    /// Placeholders and disabled text (intentionally subtle).
    pub text_faint: Rgb,
    /// The accent as chosen: buttons, selection highlights, focus rings.
    pub accent: Rgb,
    /// Accent while hovered.
    pub accent_hover: Rgb,
    /// Accent while pressed.
    pub accent_active: Rgb,
    /// A faint wash of the accent for selected rows and highlights.
    pub accent_soft: Rgb,
    /// The accent adjusted to be readable as *text* on the backgrounds
    /// (links, active tab titles).
    pub accent_text: Rgb,
    /// Text or icons drawn on top of `accent` (black or white, whichever reads best).
    pub on_accent: Rgb,
}

impl Palette {
    /// Derives a full palette from a mode and accent colour.
    pub fn new(mode: Mode, accent: Rgb) -> Self {
        let (bg_primary, bg_secondary, border, text_normal, text_muted, text_faint) = match mode {
            Mode::Dark => (
                Rgb::new(0x1B, 0x1C, 0x21),
                Rgb::new(0x15, 0x16, 0x1A),
                Rgb::new(0x2E, 0x2F, 0x36),
                Rgb::new(0xDA, 0xDA, 0xDA),
                Rgb::new(0xA0, 0xA2, 0xAA),
                Rgb::new(0x6B, 0x6D, 0x75),
            ),
            Mode::Light => (
                Rgb::new(0xFA, 0xF8, 0xF3),
                Rgb::new(0xF0, 0xEC, 0xE3),
                Rgb::new(0xDD, 0xD7, 0xCA),
                Rgb::new(0x2B, 0x2A, 0x28),
                Rgb::new(0x5E, 0x5A, 0x52),
                Rgb::new(0x8F, 0x8A, 0x7E),
            ),
        };
        let hover_step = if mode == Mode::Dark { 0.05 } else { -0.05 };
        let white = Rgb::new(255, 255, 255);
        let black = Rgb::new(0, 0, 0);
        let on_accent = if accent.contrast(white) >= accent.contrast(black) {
            white
        } else {
            black
        };
        Self {
            mode,
            bg_primary,
            bg_secondary,
            border,
            text_normal,
            text_muted,
            text_faint,
            accent,
            accent_hover: accent.shift_lightness(hover_step),
            accent_active: accent.shift_lightness(-0.06),
            accent_soft: bg_primary.mix(accent, 0.18),
            accent_text: ensure_contrast(accent, &[bg_primary, bg_secondary], mode),
            on_accent,
        }
    }
}

/// Moves `color`'s lightness (lighter on dark backgrounds, darker on light
/// ones) until it reaches [`MIN_TEXT_CONTRAST`] against every background.
fn ensure_contrast(color: Rgb, backgrounds: &[Rgb], mode: Mode) -> Rgb {
    let step = if mode == Mode::Dark { 0.01 } else { -0.01 };
    let ok = |c: Rgb| {
        backgrounds
            .iter()
            .all(|bg| c.contrast(*bg) >= MIN_TEXT_CONTRAST)
    };
    let mut current = color;
    for _ in 0..100 {
        if ok(current) {
            return current;
        }
        current = current.shift_lightness(step);
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip_and_shorthand() {
        assert_eq!(Rgb::from_hex("#C8372D").unwrap(), RUNE_RED);
        assert_eq!(Rgb::from_hex("c8372d").unwrap(), RUNE_RED);
        assert_eq!(Rgb::from_hex("  #fff ").unwrap(), Rgb::new(255, 255, 255));
        assert_eq!(Rgb::from_hex("#c33").unwrap(), Rgb::new(0xCC, 0x33, 0x33));
        assert_eq!(RUNE_RED.to_hex(), "#C8372D");
    }

    #[test]
    fn bad_hex_is_an_error_not_a_panic() {
        for bad in [
            "",
            "#",
            "#12",
            "#1234",
            "#12345",
            "#1234567",
            "#GGGGGG",
            "red",
            "#ÄÖÜ",
            "💥💥💥",
            "#ff ff00",
        ] {
            assert!(Rgb::from_hex(bad).is_err(), "{bad:?}");
        }
        let message = Rgb::from_hex("nope").unwrap_err().to_string();
        assert!(message.contains("hex code"));
    }

    #[test]
    fn contrast_matches_known_values() {
        let white = Rgb::new(255, 255, 255);
        let black = Rgb::new(0, 0, 0);
        assert!((white.contrast(black) - 21.0).abs() < 0.01);
        assert!((white.contrast(white) - 1.0).abs() < 1e-9);
        assert!((black.contrast(white) - white.contrast(black)).abs() < 1e-9);
    }

    #[test]
    fn hsl_roundtrip_is_stable() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    let c = Rgb::new(r, g, b);
                    let back = Rgb::from_hsl(c.to_hsl());
                    assert!(
                        (i16::from(c.r) - i16::from(back.r)).abs() <= 1
                            && (i16::from(c.g) - i16::from(back.g)).abs() <= 1
                            && (i16::from(c.b) - i16::from(back.b)).abs() <= 1,
                        "{c:?} -> {back:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn hsl_input_is_clamped_never_panics() {
        let weird = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e9, 1e9, 0.0];
        for h in weird {
            for s in weird {
                for l in weird {
                    let _ = Rgb::from_hsl(Hsl { h, s, l });
                }
            }
        }
    }

    #[test]
    fn every_preset_is_readable_in_both_modes() {
        for preset in ACCENT_PRESETS {
            for mode in [Mode::Dark, Mode::Light] {
                let p = Palette::new(mode, preset.color);
                for (name, color) in [("normal", p.text_normal), ("muted", p.text_muted)] {
                    for bg in [p.bg_primary, p.bg_secondary] {
                        assert!(
                            color.contrast(bg) >= MIN_TEXT_CONTRAST,
                            "{} {mode:?} {name} text: {:.2}",
                            preset.name,
                            color.contrast(bg)
                        );
                    }
                }
                for bg in [p.bg_primary, p.bg_secondary] {
                    assert!(
                        p.accent_text.contrast(bg) >= MIN_TEXT_CONTRAST,
                        "{} {mode:?} accent text: {:.2}",
                        preset.name,
                        p.accent_text.contrast(bg)
                    );
                }
                assert!(
                    p.on_accent.contrast(p.accent) >= MIN_TEXT_CONTRAST,
                    "{} {mode:?} on-accent",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn any_custom_accent_yields_a_readable_palette() {
        // Includes the awkward ones: pure black, pure white, mid grey, neon.
        let customs = [
            Rgb::new(0, 0, 0),
            Rgb::new(255, 255, 255),
            Rgb::new(128, 128, 128),
            Rgb::new(255, 0, 255),
            Rgb::new(0, 255, 0),
            Rgb::new(10, 10, 60),
        ];
        for accent in customs {
            for mode in [Mode::Dark, Mode::Light] {
                let p = Palette::new(mode, accent);
                for bg in [p.bg_primary, p.bg_secondary] {
                    assert!(
                        p.accent_text.contrast(bg) >= MIN_TEXT_CONTRAST,
                        "{accent:?} {mode:?}"
                    );
                }
                assert!(p.on_accent.contrast(p.accent) >= MIN_TEXT_CONTRAST);
            }
        }
    }

    #[test]
    fn default_accent_is_rune_red_and_stays_itself() {
        assert_eq!(ACCENT_PRESETS[0].name, "Rune Red");
        assert_eq!(ACCENT_PRESETS[0].color, RUNE_RED);
        assert_eq!(Palette::new(Mode::Dark, RUNE_RED).accent, RUNE_RED);
    }

    #[test]
    fn changing_accent_changes_only_accent_derived_colours() {
        let red = Palette::new(Mode::Dark, RUNE_RED);
        let blue = Palette::new(Mode::Dark, ACCENT_PRESETS[4].color);
        assert_eq!(red.bg_primary, blue.bg_primary);
        assert_eq!(red.text_normal, blue.text_normal);
        assert_ne!(red.accent, blue.accent);
        assert_ne!(red.accent_soft, blue.accent_soft);
    }
}
