use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    BlockedFs,
    BlueNoise,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Solid { r: u8, g: u8, b: u8, a: u8 },
    GradientH,
    GradientV,
    GradientDiag,
    GradientRadial,
    RampR,
    RampG,
    RampB,
    Checker,
    Noise { seed: u64 },
    BwEdges,
    BwStripes,
    AllZero,
    AllMax,
    OutOfGamut,
    AccumulatingError,
    AlphaVarying,
    Photo { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    pub pixel_size: u32,
    pub spread: u32,
    pub pad: u32,
    pub texture_version: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            pixel_size: 1,
            spread: 256,
            pad: 8,
            texture_version: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ParityCase {
    pub id: String,
    pub algorithm: Algorithm,
    pub width: u32,
    pub height: u32,
    pub palette: Vec<[u8; 4]>,
    pub input: InputKind,
    pub seed: u64,
    pub params: Params,
    pub group: String,
    /// Coverage tags for §9 inventory (ties, oog, edge sizes, …).
    pub tags: Vec<String>,
}

impl ParityCase {
    pub fn make_id(algo: Algorithm, w: u32, h: u32, pal_tag: &str, input_tag: &str, seed: u64) -> String {
        let a = match algo {
            Algorithm::BlockedFs => "bfs",
            Algorithm::BlueNoise => "bn",
        };
        format!("{a}_{w}x{h}_{pal_tag}_{input_tag}_s{seed:x}")
    }
}
