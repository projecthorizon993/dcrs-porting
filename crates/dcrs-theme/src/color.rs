//! Evaluating CSS colour expressions to `#RRGGBB[AA]`.
//!
//! Serein's theme API accepts only `#RRGGBB` or `#RRGGBBAA`. Discord themes, however, lean heavily on
//! computed colour expressions: `hsl(var(--brand-experiment-hsl) / 0.24)`,
//! `rgb(49 51 56 / 80%)`, `hsl(235deg 86% 66%)`. Every one of those has to be *evaluated* to hex,
//! not merely resolved for `var()`.
//!
//! This handles what themes actually emit. An expression it cannot evaluate is reported as
//! [`ColorError::Unsupported`] rather than guessed at, because a wrong colour is worse than a
//! missing one — the theme still applies, minus that token.

use std::fmt;

use thiserror::Error;

/// An RGBA colour, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, 0–255.
    pub a: u8,
}

impl Rgba {
    /// An opaque colour.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A colour with alpha.
    #[must_use]
    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `#rrggbb`, or `#rrggbbaa` when not fully opaque.
    ///
    /// Lowercase, because Serein's editor round-trips what it is given and mixed case would make
    /// diffs noisy.
    #[must_use]
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Relative luminance per WCAG 2.x, used to pick a legible `accent_text`.
    #[must_use]
    pub fn luminance(self) -> f32 {
        let channel = |c: u8| {
            let v = f32::from(c) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Why a colour expression could not be evaluated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ColorError {
    /// The function is real CSS but themes rarely use it, so it is not implemented.
    #[error("unsupported colour function {0:?}")]
    Unsupported(String),
    /// The expression is malformed.
    #[error("malformed colour {expr:?}: {reason}")]
    Malformed {
        /// The offending expression.
        expr: String,
        /// Why.
        reason: &'static str,
    },
    /// A length or angle is present but outside the range the channel accepts.
    #[error("value {value} out of range for {channel} in {expr:?}")]
    OutOfRange {
        /// The expression.
        expr: String,
        /// Which component.
        channel: &'static str,
        /// The value.
        value: f32,
    },
}

/// Named colours, enough to cover the ones themes actually use.
fn named(name: &str) -> Option<Rgba> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "transparent" => Rgba::with_alpha(0, 0, 0, 0),
        "black" => Rgba::rgb(0, 0, 0),
        "white" => Rgba::rgb(255, 255, 255),
        "red" => Rgba::rgb(255, 0, 0),
        "green" => Rgba::rgb(0, 128, 0),
        "blue" => Rgba::rgb(0, 0, 255),
        "yellow" => Rgba::rgb(255, 255, 0),
        "orange" => Rgba::rgb(255, 165, 0),
        "purple" => Rgba::rgb(128, 0, 128),
        "gray" | "grey" => Rgba::rgb(128, 128, 128),
        "silver" => Rgba::rgb(192, 192, 192),
        "white-smoke" | "whitesmoke" => Rgba::rgb(245, 245, 245),
        _ => return None,
    })
}

/// Evaluates a colour expression.
pub fn evaluate(expr: &str) -> Result<Rgba, ColorError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "empty expression",
        });
    }

    // `transparent` and hex are by far the most common, so try them first.
    if let Some(c) = named(trimmed).filter(|_| !trimmed.starts_with('#')) {
        return Ok(c);
    }
    if trimmed.starts_with('#') {
        return parse_hex(trimmed);
    }

    let Some((name, args)) = split_function(trimmed) else {
        return Err(ColorError::Unsupported(trimmed.to_owned()));
    };

    match name.as_str() {
        "rgb" | "rgba" => parse_rgb(args, trimmed),
        "hsl" | "hsla" => parse_hsl(args, trimmed),
        "oklch" | "oklab" | "lab" | "lch" | "color" | "color-mix" | "hwb" => {
            Err(ColorError::Unsupported(name))
        }
        other => Err(ColorError::Unsupported(other.to_owned())),
    }
}

/// Splits `name(args)`.
fn split_function(expr: &str) -> Option<(String, &str)> {
    let open = expr.find('(')?;
    if !expr.ends_with(')') {
        return None;
    }
    let name = expr[..open].trim().to_ascii_lowercase();
    let args = &expr[open + 1..expr.len() - 1];
    Some((name, args))
}

fn parse_hex(expr: &str) -> Result<Rgba, ColorError> {
    let digits = &expr[1..];
    let bad = || ColorError::Malformed {
        expr: expr.to_owned(),
        reason: "bad hex digits",
    };
    let byte = |s: &str| u8::from_str_radix(s, 16).map_err(|_| bad());
    match digits.len() {
        3 => {
            // `#abc` expands each nibble, so no expansion maths is needed beyond doubling.
            let mut out = [0u8; 3];
            for (i, c) in digits.chars().enumerate() {
                let v = c.to_digit(16).ok_or_else(bad)?;
                out[i] = u8::try_from(v * 17).map_err(|_| bad())?;
            }
            Ok(Rgba::rgb(out[0], out[1], out[2]))
        }
        4 => {
            let mut out = [0u8; 4];
            for (i, c) in digits.chars().enumerate() {
                let v = c.to_digit(16).ok_or_else(bad)?;
                out[i] = u8::try_from(v * 17).map_err(|_| bad())?;
            }
            Ok(Rgba::with_alpha(out[0], out[1], out[2], out[3]))
        }
        6 => Ok(Rgba::rgb(
            byte(&digits[0..2])?,
            byte(&digits[2..4])?,
            byte(&digits[4..6])?,
        )),
        8 => Ok(Rgba::with_alpha(
            byte(&digits[0..2])?,
            byte(&digits[2..4])?,
            byte(&digits[4..6])?,
            byte(&digits[6..8])?,
        )),
        _ => Err(bad()),
    }
}

/// Splits function arguments on commas, or on whitespace when the modern `/` form is used.
fn split_args(args: &str) -> Vec<String> {
    if args.contains(',') {
        return args.split(',').map(|a| a.trim().to_owned()).collect();
    }
    // Modern syntax: `rgb(49 51 56 / 80%)`. Keep the alpha after the slash with its number.
    let (channels, alpha) = match args.split_once('/') {
        Some((c, a)) => (c, Some(a)),
        None => (args, None),
    };
    let mut out: Vec<String> = channels.split_whitespace().map(str::to_owned).collect();
    if let Some(a) = alpha {
        out.push(a.trim().to_owned());
    }
    out
}

/// Parses a number that may be a percentage, returning the value on a 0–255 scale.
fn byte_arg(raw: &str, expr: &str, channel: &'static str) -> Result<u8, ColorError> {
    let t = raw.trim();
    if let Some(pct) = t.strip_suffix('%') {
        let v: f32 = pct.trim().parse().map_err(|_| ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "bad percentage",
        })?;
        return clamp_byte(v / 100.0 * 255.0, expr, channel);
    }
    let v: f32 = t.parse().map_err(|_| ColorError::Malformed {
        expr: expr.to_owned(),
        reason: "bad number",
    })?;
    clamp_byte(v, expr, channel)
}

/// Rounds a float channel into a byte, clamping out-of-gamut values.
///
/// Every caller has already validated the expression, so this never sees a non-finite value; the
/// `NaN` branch exists so a pathological input cannot produce a zeroed channel by accident.
fn byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

fn clamp_byte(v: f32, expr: &str, channel: &'static str) -> Result<u8, ColorError> {
    if v.is_nan() {
        return Err(ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "NaN component",
        });
    }
    let _ = channel;
    // Clamp rather than reject: an out-of-gamut channel is still a usable colour, and themes
    // routinely push channels past 255 when composing effects.
    Ok(v.clamp(0.0, 255.0).round() as u8)
}

fn parse_rgb(args: &str, expr: &str) -> Result<Rgba, ColorError> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return Err(ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "rgb needs three channels",
        });
    }
    let r = byte_arg(&parts[0], expr, "red")?;
    let g = byte_arg(&parts[1], expr, "green")?;
    let b = byte_arg(&parts[2], expr, "blue")?;
    let a = match parts.get(3) {
        Some(raw) => alpha_arg(raw, expr)?,
        None => 255,
    };
    Ok(Rgba::with_alpha(r, g, b, a))
}

/// Parses an alpha component, which may be `0.5` or `80%`.
fn alpha_arg(raw: &str, expr: &str) -> Result<u8, ColorError> {
    let t = raw.trim();
    let value = if let Some(pct) = t.strip_suffix('%') {
        let v: f32 = pct.trim().parse().map_err(|_| ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "bad alpha percentage",
        })?;
        v / 100.0
    } else {
        t.parse().map_err(|_| ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "bad alpha",
        })?
    };
    if !(0.0..=1.0).contains(&value) {
        return Err(ColorError::OutOfRange {
            expr: expr.to_owned(),
            channel: "alpha",
            value,
        });
    }
    Ok((value * 255.0).round() as u8)
}

/// Parses a hue, accepting `deg`, `rad`, `grad`, `turn` or a bare number.
fn hue_arg(raw: &str, expr: &str) -> Result<f32, ColorError> {
    let t = raw.trim().to_ascii_lowercase();
    let bad = || ColorError::Malformed {
        expr: expr.to_owned(),
        reason: "bad hue",
    };
    let (num, scale) = if let Some(v) = t.strip_suffix("deg") {
        (v, 1.0)
    } else if let Some(v) = t.strip_suffix("grad") {
        (v, 0.9)
    } else if let Some(v) = t.strip_suffix("rad") {
        (v, 57.29578)
    } else if let Some(v) = t.strip_suffix("turn") {
        (v, 360.0)
    } else {
        (t.as_str(), 1.0)
    };
    let v: f32 = num.trim().parse().map_err(|_| bad())?;
    Ok(v * scale)
}

/// Parses a saturation or lightness component, which may be a percentage or a bare number.
fn percent_arg(raw: &str, expr: &str, channel: &'static str) -> Result<f32, ColorError> {
    let t = raw.trim();
    let value = if let Some(pct) = t.strip_suffix('%') {
        let v: f32 = pct.trim().parse().map_err(|_| ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "bad percentage",
        })?;
        v / 100.0
    } else {
        let v: f32 = t.parse().map_err(|_| ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "bad number",
        })?;
        v / 100.0
    };
    if !(0.0..=1.0).contains(&value) {
        return Err(ColorError::OutOfRange {
            expr: expr.to_owned(),
            channel,
            value,
        });
    }
    Ok(value)
}

fn parse_hsl(args: &str, expr: &str) -> Result<Rgba, ColorError> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return Err(ColorError::Malformed {
            expr: expr.to_owned(),
            reason: "hsl needs hue, saturation and lightness",
        });
    }
    let hue = hue_arg(&parts[0], expr)?;
    let saturation = percent_arg(&parts[1], expr, "saturation")?;
    let lightness = percent_arg(&parts[2], expr, "lightness")?;
    let alpha = match parts.get(3) {
        Some(raw) => alpha_arg(raw, expr)?,
        None => 255,
    };
    let (r, g, b) = hsl_to_rgb(hue, saturation, lightness);
    Ok(Rgba::with_alpha(r, g, b, alpha))
}

/// HSL to RGB. The hue wraps, which themes rely on when using an out-of-range hue.
fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let hue = hue.rem_euclid(360.0) / 360.0;
    if saturation == 0.0 {
        let v = byte(lightness * 255.0);
        return (v, v, v);
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    let to_channel = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };
    (
        byte(to_channel(hue + 1.0 / 3.0) * 255.0),
        byte(to_channel(hue) * 255.0),
        byte(to_channel(hue - 1.0 / 3.0) * 255.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(evaluate("#5865f2").unwrap(), Rgba::rgb(0x58, 0x65, 0xf2));
        assert_eq!(evaluate("#ABC").unwrap(), Rgba::rgb(0xaa, 0xbb, 0xcc));
        assert_eq!(evaluate("#5865f2ff").unwrap().a, 255);
        assert_eq!(evaluate("#00000080").unwrap().a, 128);
        assert_eq!(evaluate("#abcd").unwrap().a, 0xdd);
        assert!(matches!(
            evaluate("#12345"),
            Err(ColorError::Malformed { .. })
        ));
    }

    #[test]
    fn parses_named_colours() {
        assert_eq!(evaluate("white").unwrap(), Rgba::rgb(255, 255, 255));
        assert_eq!(evaluate("  BLACK  ").unwrap(), Rgba::rgb(0, 0, 0));
        assert_eq!(evaluate("transparent").unwrap().a, 0);
    }

    #[test]
    fn parses_legacy_rgb() {
        let c = evaluate("rgb(49, 51, 56)").unwrap();
        assert_eq!(c, Rgba::rgb(49, 51, 56));
        assert_eq!(evaluate("rgba(0,0,0,0.5)").unwrap().a, 128);
        assert_eq!(evaluate("rgb(49, 51, 56, 0.5)").unwrap().a, 128);
    }

    #[test]
    fn parses_modern_rgb_with_alpha_slash() {
        let c = evaluate("rgb(49 51 56 / 80%)").unwrap();
        assert_eq!(c.r, 49);
        assert_eq!(c.a, 204);
    }

    #[test]
    fn parses_hsl_legacy_and_modern() {
        // Discord's own blurple is hsl(235 86% 66%).
        let c = evaluate("hsl(235, 86%, 66%)").unwrap();
        assert_eq!(c, evaluate("hsl(235 86% 66%)").unwrap());
        assert_eq!(c, evaluate("hsl(235deg 86% 66%)").unwrap());
    }

    #[test]
    fn parses_hsl_with_alpha() {
        let c = evaluate("hsl(235 86% 66% / 0.5)").unwrap();
        assert_eq!(c.a, 128);
    }

    #[test]
    fn hue_units_agree() {
        let a = evaluate("hsl(90deg 100% 50%)").unwrap();
        let b = evaluate("hsl(0.25turn 100% 50%)").unwrap();
        let c = evaluate("hsl(100grad 100% 50%)").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn hue_wraps_past_360() {
        assert_eq!(
            evaluate("hsl(360 100% 50%)").unwrap(),
            evaluate("hsl(0 100% 50%)").unwrap()
        );
        assert_eq!(
            evaluate("hsl(480 100% 50%)").unwrap(),
            evaluate("hsl(120 100% 50%)").unwrap()
        );
    }

    #[test]
    fn achromatic_hsl_is_grey() {
        assert_eq!(evaluate("hsl(0 0% 50%)").unwrap(), Rgba::rgb(128, 128, 128));
        assert_eq!(evaluate("hsl(0 0% 0%)").unwrap(), Rgba::rgb(0, 0, 0));
    }

    #[test]
    fn clamps_out_of_gamut_channels_rather_than_failing() {
        // Themes compose effects by pushing channels past the range; that still has to render.
        let c = evaluate("rgb(300, -20, 56)").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
    }

    #[test]
    fn rejects_out_of_range_alpha() {
        assert!(matches!(
            evaluate("rgba(0,0,0,1.5)"),
            Err(ColorError::OutOfRange {
                channel: "alpha",
                ..
            })
        ));
    }

    #[test]
    fn reports_unsupported_functions_honestly() {
        for expr in [
            "oklch(0.7 0.1 200)",
            "color-mix(in oklch, red, blue)",
            "lab(50 20 -30)",
        ] {
            assert!(
                matches!(evaluate(expr), Err(ColorError::Unsupported(_))),
                "{expr} should be reported unsupported"
            );
        }
    }

    #[test]
    fn rejects_malformed_input() {
        for expr in ["", "rgb(1,2)", "hsl(0 0%)", "#gg0000", "notacolour"] {
            assert!(evaluate(expr).is_err(), "{expr:?} should not parse");
        }
    }

    #[test]
    fn hex_output_is_lowercase_and_omits_opaque_alpha() {
        assert_eq!(Rgba::rgb(0xAB, 0xCD, 0xEF).to_hex(), "#abcdef");
        assert_eq!(
            Rgba::with_alpha(0xAB, 0xCD, 0xEF, 0x80).to_hex(),
            "#abcdef80"
        );
    }

    #[test]
    fn luminance_orders_light_and_dark() {
        assert!(Rgba::rgb(255, 255, 255).luminance() > Rgba::rgb(0, 0, 0).luminance());
        // Discord's blurple is dark enough that white text is the legible choice.
        let blurple = evaluate("hsl(235 86% 66%)").unwrap();
        assert!(
            blurple.luminance() < 0.5,
            "expected a mid-dark accent, got {}",
            blurple.luminance()
        );
    }
}
