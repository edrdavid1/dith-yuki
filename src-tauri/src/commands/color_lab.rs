use std::sync::Arc;
use tauri::State;

use serde::{Deserialize, Serialize};

use crate::commands::AppState;
use crate::services::palette_service::{hex_to_linear, linear_to_hex};

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedColorDto {
    pub hex: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

fn normalize_hex_arg(hex: &str) -> Result<String, String> {
    let trimmed = hex.trim().trim_start_matches('#').to_uppercase();
    if trimmed.len() != 6 {
        return Err("Hex color must be exactly 6 characters (optionally prefixed with #)".to_string());
    }
    if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Hex color contains invalid characters".to_string());
    }
    Ok(trimmed)
}

fn lin_rgb_to_generated(c: engine_color::LinRgb) -> GeneratedColorDto {
    use engine_color::palette::{linear_to_srgb, LinearColor};
    let lc = LinearColor {
        r: c.r,
        g: c.g,
        b: c.b,
    };
    let hex = linear_to_hex(&lc);
    GeneratedColorDto {
        hex: format!("#{}", hex),
        r: linear_to_srgb(c.r),
        g: linear_to_srgb(c.g),
        b: linear_to_srgb(c.b),
    }
}

fn hex_arg_to_lin_rgb(hex: &str) -> Result<engine_color::LinRgb, String> {
    let normalized = normalize_hex_arg(hex)?;
    let lc = hex_to_linear(&normalized)?;
    Ok(engine_color::LinRgb {
        r: lc.r,
        g: lc.g,
        b: lc.b,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct OklabPointDto {
    pub l: f32,
    pub a: f32,
    pub b: f32,
    pub srgb_hex: String,
}

fn oklab_point_from_lin_rgb(rgb: engine_color::LinRgb, srgb_hex: String) -> OklabPointDto {
    let lab = engine_color::linear_to_oklab(rgb);
    OklabPointDto {
        l: lab.l,
        a: lab.a,
        b: lab.b,
        srgb_hex,
    }
}

pub fn oklab_points_from_hexes(colors: &[String]) -> Result<Vec<OklabPointDto>, String> {
    colors
        .iter()
        .map(|hex| {
            let rgb = hex_arg_to_lin_rgb(hex)?;
            let normalized = normalize_hex_arg(hex)?;
            Ok(oklab_point_from_lin_rgb(rgb, format!("#{}", normalized)))
        })
        .collect()
}

pub fn oklab_points_from_linear(
    colors: &[engine_color::palette::LinearColor],
) -> Vec<OklabPointDto> {
    colors
        .iter()
        .map(|c| {
            let rgb = engine_color::LinRgb {
                r: c.r,
                g: c.g,
                b: c.b,
            };
            oklab_point_from_lin_rgb(rgb, format!("#{}", linear_to_hex(c)))
        })
        .collect()
}

#[tauri::command]
pub fn generate_ramp_palette(
    from_hex: String,
    to_hex: String,
    steps: u32,
) -> Result<Vec<GeneratedColorDto>, String> {
    if !(1..=64).contains(&steps) {
        return Err("steps must be between 1 and 64".to_string());
    }
    let from = hex_arg_to_lin_rgb(&from_hex)?;
    let to = hex_arg_to_lin_rgb(&to_hex)?;
    let ramp = engine_color::generate_ramp(from, to, steps as usize);
    Ok(ramp.into_iter().map(lin_rgb_to_generated).collect())
}

#[tauri::command]
pub fn generate_harmony_palette(
    base_hex: String,
    rule: String,
    count: u32,
    analogous_spread: Option<f32>,
) -> Result<Vec<GeneratedColorDto>, String> {
    use engine_color::HarmonyRule;

    if !(1..=32).contains(&count) {
        return Err("count must be between 1 and 32".to_string());
    }
    let harmony_rule = match rule.as_str() {
        "Monochromatic" => HarmonyRule::Monochromatic,
        "Analogous" => HarmonyRule::Analogous,
        "Complementary" => HarmonyRule::Complementary,
        "Triadic" => HarmonyRule::Triadic,
        "SplitComplementary" => HarmonyRule::SplitComplementary,
        other => {
            return Err(format!(
                "Unknown harmony rule '{}'. Expected Monochromatic|Analogous|Complementary|Triadic|SplitComplementary",
                other
            ))
        }
    };
    let base = hex_arg_to_lin_rgb(&base_hex)?;
    let colors = if let Some(spread) = analogous_spread {
        if !spread.is_finite() || spread < 0.0 || spread > std::f32::consts::PI {
            return Err("analogous_spread must be a finite value in [0, π] radians".to_string());
        }
        engine_color::generate_harmony_with_spread(base, harmony_rule, count as usize, spread)
    } else {
        engine_color::generate_harmony(base, harmony_rule, count as usize)
    };
    Ok(colors.into_iter().map(lin_rgb_to_generated).collect())
}

#[tauri::command]
pub fn colors_to_oklab(colors: Vec<String>) -> Result<Vec<OklabPointDto>, String> {
    oklab_points_from_hexes(&colors)
}

#[tauri::command]
pub fn get_palette_oklab(
    palette_id: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<OklabPointDto>, String> {
    let snapshot = state.active_session()?.document_handle.snapshot();
    let palette = snapshot
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .ok_or_else(|| format!("Palette {} not found", palette_id))?;
    Ok(oklab_points_from_linear(&palette.colors))
}
