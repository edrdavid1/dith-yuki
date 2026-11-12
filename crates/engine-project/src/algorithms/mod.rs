//! Built-in algorithm implementations of the `FilterAlgorithm` trait.
//!
//! Call [`register_all`] at application start (or via [`builtin_registry`]) to
//! populate an `AlgorithmRegistry` with every compiled-in algorithm.

use std::sync::OnceLock;

use engine_registry::AlgorithmRegistry;

mod adjust;
mod ascii;
mod bayer;
pub mod blue_noise_texture;
mod blur_noise;
mod clarity;
mod clustered_dot_ordered;
mod cmyk_halftone;
mod crosshatch_dither;
mod crt;
mod cube_lut;
mod curves;
mod detail;
mod dispersed_dot_ordered;
mod error_diffusion;
mod exposure;
pub mod fixed_point;
mod glitch;
mod glow;
pub mod gpu_dither;
pub mod gpu_dither_cpu;
pub mod gpu_geometry;
pub mod gpu_selftest;
mod halftone_screen_angled;
mod highlights_shadows;
mod hue;
mod invert;
mod levels;
mod line_screen;
mod nearest_color;
mod palette_quantize;
mod posterize;
mod random_dot_stipple;
mod riemersma;
mod scratch;
mod temperature_tint;
mod threshold;
mod vibrance;
mod void_and_cluster;
mod voronoi_stipple;
mod wave;

/// Process-wide built-in registry (Req 1.5).
///
/// Populated once on first use. Tile apply uses this so public wrappers do not
/// need a new `registry` argument at every call site.
pub fn builtin_registry() -> &'static AlgorithmRegistry {
    static REGISTRY: OnceLock<AlgorithmRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = AlgorithmRegistry::new();
        register_all(&mut registry);
        registry
    })
}

/// Register all built-in algorithms into `registry`.
///
/// Called once at application start. Additional algorithms are registered here
/// as each Phase 2 migration task completes.
pub fn register_all(registry: &mut AlgorithmRegistry) {
    registry.register(Box::new(bayer::Bayer2x2));
    registry.register(Box::new(bayer::Bayer4x4));
    registry.register(Box::new(bayer::Bayer8x8));
    registry.register(Box::new(bayer::Bayer16x16));
    registry.register(Box::new(clustered_dot_ordered::ClusteredDotOrdered));
    registry.register(Box::new(crosshatch_dither::CrosshatchDither));
    registry.register(Box::new(dispersed_dot_ordered::DispersedDotOrdered));
    registry.register(Box::new(void_and_cluster::VoidAndCluster));
    registry.register(Box::new(voronoi_stipple::VoronoiStipple));
    registry.register(Box::new(random_dot_stipple::RandomDotStipple));
    registry.register(Box::new(palette_quantize::PaletteQuantizeAlgo));
    registry.register(Box::new(nearest_color::NearestColorAlgo));
    registry.register(Box::new(cmyk_halftone::CmykHalftone));
    registry.register(Box::new(halftone_screen_angled::HalftoneScreenAngled));
    registry.register(Box::new(line_screen::LineScreen));
    registry.register(Box::new(gpu_dither::BlockedFs));
    registry.register(Box::new(gpu_dither::BlueNoise));
    registry.register(Box::new(error_diffusion::FloydSteinberg));
    registry.register(Box::new(error_diffusion::Atkinson));
    registry.register(Box::new(error_diffusion::JarvisJudiceNinke));
    registry.register(Box::new(error_diffusion::Ostromoukhov));
    registry.register(Box::new(riemersma::Riemersma));
    registry.register(Box::new(error_diffusion::Stucki));
    registry.register(Box::new(error_diffusion::Burkes));
    registry.register(Box::new(error_diffusion::Fan93));
    registry.register(Box::new(error_diffusion::Sierra));
    registry.register(Box::new(error_diffusion::SierraLite));
    registry.register(Box::new(error_diffusion::SierraTwoRow));
    registry.register(Box::new(error_diffusion::ShiauFan));
    registry.register(Box::new(error_diffusion::StevensonArce));
    registry.register(Box::new(wave::Wave));
    registry.register(Box::new(crt::Crt));
    registry.register(Box::new(glow::Glow));
    registry.register(Box::new(adjust::Adjust));
    registry.register(Box::new(ascii::Ascii));
    registry.register(Box::new(curves::Curves));
    registry.register(Box::new(exposure::Exposure));
    registry.register(Box::new(highlights_shadows::HighlightsShadows));
    registry.register(Box::new(temperature_tint::TemperatureTint));
    registry.register(Box::new(vibrance::Vibrance));
    registry.register(Box::new(hue::Hue));
    registry.register(Box::new(clarity::Clarity));
    registry.register(Box::new(detail::Detail));
    registry.register(Box::new(blur_noise::BlurNoise));
    registry.register(Box::new(invert::Invert));
    registry.register(Box::new(threshold::Threshold));
    registry.register(Box::new(posterize::Posterize));
    registry.register(Box::new(levels::Levels));
    registry.register(Box::new(cube_lut::CubeLut));
    registry.register(Box::new(glitch::Glitch));
    registry.register(Box::new(error_diffusion::ZhouFang));
    registry.register(Box::new(error_diffusion::ModulatedDiffuseY));
    registry.register(Box::new(error_diffusion::ModulatedDiffuseX));
    registry.register(Box::new(error_diffusion::GlitchDither));
    registry.register(Box::new(error_diffusion::ContrastAwareY));
    registry.register(Box::new(error_diffusion::ContrastAwareX));
    registry.register(Box::new(error_diffusion::SmoothDiffuse));
    registry.register(Box::new(error_diffusion::WaveModulationY));
    registry.register(Box::new(error_diffusion::WaveModulationX));
}
