//! Built-in algorithm implementations of the `FilterAlgorithm` trait.
//!
//! Call [`register_all`] at application start (or via [`builtin_registry`]) to
//! populate an `AlgorithmRegistry` with every compiled-in algorithm.

use std::sync::OnceLock;

use engine_registry::AlgorithmRegistry;

mod adjust;
mod bayer;
mod clustered_dot_ordered;
mod cmyk_halftone;
mod crt;
mod curves;
mod dispersed_dot_ordered;
mod error_diffusion;
mod glitch;
mod glow;
mod halftone_screen_angled;
mod palette_quantize;
mod scratch;
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
    registry.register(Box::new(dispersed_dot_ordered::DispersedDotOrdered));
    registry.register(Box::new(palette_quantize::PaletteQuantizeAlgo));
    registry.register(Box::new(cmyk_halftone::CmykHalftone));
    registry.register(Box::new(halftone_screen_angled::HalftoneScreenAngled));
    registry.register(Box::new(error_diffusion::FloydSteinberg));
    registry.register(Box::new(error_diffusion::Atkinson));
    registry.register(Box::new(error_diffusion::JarvisJudiceNinke));
    registry.register(Box::new(error_diffusion::Ostromoukhov));
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
    registry.register(Box::new(curves::Curves));
    registry.register(Box::new(glitch::Glitch));
}
