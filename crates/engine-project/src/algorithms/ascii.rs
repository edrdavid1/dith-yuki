//! ASCII / text-art effect (`ascii`) — not a dither mode.
//!
//! Product path: dedicated [`FilterKind::Ascii`] + EffectCategory::Ascii.
//! Execution is [`ExecutionScope::FullDocument`]: the tile pipeline runs
//! [`crate::filters::ascii_job`] and publishes Processed tiles (preview raster).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, ExecutionScope, FilterAlgorithm, FilterCtx,
    FilterError, GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filter::AsciiParams;

const SCHEMA: &[ParamField] = &[
    ParamField::Dropdown {
        key: "font",
        label: "Font",
        options: &[
            ("departure_mono", "Departure Mono"),
            ("ibm_plex_mono", "IBM Plex Mono"),
        ],
        default: "departure_mono",
    },
    ParamField::Dropdown {
        key: "size_mode",
        label: "Size Mode",
        options: &[("px", "Font size (px)"), ("columns", "Columns")],
        default: "px",
    },
    ParamField::Slider {
        key: "font_px",
        label: "Font Size",
        min: 4.0,
        max: 64.0,
        default: 11.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "columns",
        label: "Columns",
        min: 20.0,
        max: 320.0,
        default: 120.0,
        step: Some(1.0),
    },
    ParamField::Checkbox {
        key: "antialias",
        label: "Antialias",
        default: false,
    },
    ParamField::Checkbox {
        key: "hinting",
        label: "Hinting",
        default: false,
    },
    ParamField::Dropdown {
        key: "symbol_set",
        label: "Symbol Set",
        options: &[
            ("bourke_10", "Bourke 10"),
            ("bourke_70", "Bourke 70"),
            ("printable_ascii", "Printable ASCII"),
            ("ascii_box_drawing", "ASCII + Box Drawing"),
            ("blocks", "Blocks"),
            ("quadrants", "Quadrants"),
            ("sextants", "Sextants"),
            ("octants", "Octants"),
            ("braille", "Braille"),
            ("cp437", "CP437"),
        ],
        default: "bourke_70",
    },
    ParamField::Dropdown {
        key: "match_mode",
        label: "Match Mode",
        options: &[
            ("tone", "Tone"),
            ("shape", "Shape"),
            ("shape_contrast", "Shape + Contrast"),
            ("mask_two_color", "Mask Two-Color"),
        ],
        default: "shape",
    },
    ParamField::Slider {
        key: "contrast",
        label: "Contrast",
        min: 0.25,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[
            ("mono", "Mono"),
            ("fg", "Foreground"),
            ("fg_bg", "Foreground + Background"),
        ],
        default: "mono",
    },
    ParamField::Dropdown {
        key: "color_target",
        label: "Color Target",
        options: &[
            ("truecolor", "TrueColor"),
            ("xterm256", "xterm 256"),
            ("ansi16_vga", "ANSI-16 VGA"),
            ("ansi16_xterm", "ANSI-16 xterm"),
            ("ansi16_win10", "ANSI-16 Windows 10"),
        ],
        default: "truecolor",
    },
    ParamField::Dropdown {
        key: "cell_dither",
        label: "Cell Dither",
        options: &[
            ("none", "None"),
            ("bayer2", "Bayer 2×2"),
            ("bayer4", "Bayer 4×4"),
            ("bayer8", "Bayer 8×8"),
            ("floyd_steinberg", "Floyd–Steinberg"),
        ],
        default: "none",
    },
    ParamField::Checkbox {
        key: "serpentine",
        label: "Serpentine",
        default: false,
    },
    ParamField::Checkbox {
        key: "edge_overlay",
        label: "Edge Overlay",
        default: false,
    },
    ParamField::Slider {
        key: "edge_tau",
        label: "Edge Threshold",
        min: 0.0,
        max: 255.0,
        default: 40.0,
        step: Some(1.0),
    },
];

pub struct Ascii;

impl FilterAlgorithm for Ascii {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("ascii")
    }

    fn display_name(&self) -> &'static str {
        "ASCII"
    }

    fn apply(
        &self,
        _tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        // Tile path is unused — full-document job owns pixels. Still validate.
        let _: AsciiParams = serde_json::from_value(params.clone())?;
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn execution_scope(&self) -> ExecutionScope {
        ExecutionScope::FullDocument
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Ascii
    }
}
