//! Curves filter implementation.
//!
//! Tone adjustment via Catmull-Rom spline interpolation.

use crate::error::EngineError;
use engine_tiles::PixelTile;
use serde::{Deserialize, Serialize};

/// Which channel(s) to apply the curve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveChannel {
    /// Apply to red channel only
    Red,
    /// Apply to green channel only
    Green,
    /// Apply to blue channel only
    Blue,
    /// Apply to luminance (Y in Lab color space)
    Luminance,
    /// Apply to all RGB channels independently
    All,
}

/// Curves filter for tone adjustment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvesFilter {
    /// Control points (input, output) in range [0, 1]
    pub curve: Vec<(f32, f32)>,
    /// Which channel to apply the curve to
    pub channel: CurveChannel,
}

impl CurvesFilter {
    /// Create a new curves filter with default linear curve.
    pub fn new(channel: CurveChannel) -> Self {
        CurvesFilter {
            curve: vec![(0.0, 0.0), (1.0, 1.0)],
            channel,
        }
    }

    /// Add or update a control point.
    pub fn add_point(&mut self, input: f32, output: f32) -> Result<(), EngineError> {
        if !(0.0..=1.0).contains(&input) || !(0.0..=1.0).contains(&output) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Curve control point out of range [0, 1]".to_string(),
            });
        }

        // Find or insert point at input
        if let Some(pos) = self.curve.iter().position(|(x, _)| (x - input).abs() < 0.001) {
            self.curve[pos] = (input, output);
        } else {
            self.curve.push((input, output));
            self.curve.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        }

        Ok(())
    }

    /// Evaluate the curve at a given input value using Catmull-Rom interpolation.
    pub fn evaluate(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);

        // If x is at endpoints, return directly
        if (x - 0.0).abs() < 0.001 {
            return self.curve[0].1.clamp(0.0, 1.0);
        }
        if (x - 1.0).abs() < 0.001 {
            return self.curve[self.curve.len() - 1].1.clamp(0.0, 1.0);
        }

        // Find bracketing points
        let mut i = 0;
        while i < self.curve.len() - 1 && self.curve[i + 1].0 <= x {
            i += 1;
        }

        // Get the 4 control points for Catmull-Rom
        let p0 = if i == 0 {
            (self.curve[0].0 - (self.curve[1].0 - self.curve[0].0), self.curve[0].1)
        } else {
            self.curve[i - 1]
        };

        let p1 = self.curve[i];
        let p2 = self.curve[i + 1];

        let p3 = if i + 2 < self.curve.len() {
            self.curve[i + 2]
        } else {
            (self.curve[self.curve.len() - 1].0 + (self.curve[self.curve.len() - 1].0 - self.curve[self.curve.len() - 2].0), self.curve[self.curve.len() - 1].1)
        };

        // Normalize t to [0, 1] between p1 and p2
        let t = (x - p1.0) / (p2.0 - p1.0);
        let t = t.clamp(0.0, 1.0);

        // Catmull-Rom interpolation
        let t2 = t * t;
        let t3 = t2 * t;

        let a0 = -0.5 * p0.1 + 1.5 * p1.1 - 1.5 * p2.1 + 0.5 * p3.1;
        let a1 = p0.1 - 2.5 * p1.1 + 2.0 * p2.1 - 0.5 * p3.1;
        let a2 = -0.5 * p0.1 + 0.5 * p2.1;
        let a3 = p1.1;

        let result = a0 * t3 + a1 * t2 + a2 * t + a3;
        result.clamp(0.0, 1.0)
    }

    /// Apply the curves filter to a tile.
    pub fn apply_to_tile(&self, tile: &PixelTile) -> Result<PixelTile, EngineError> {
        let mut result = PixelTile::new();

        // Copy all pixels from source and apply curve transformation
        for y in 0u32..260 {
            for x in 0u32..260 {
                // Copy alpha channel unchanged
                result.set(x, y, 3, tile.at(x, y, 3));

                // Apply curve to relevant channels
                match self.channel {
                    CurveChannel::Red => {
                        let r = tile.at(x, y, 0);
                        result.set(x, y, 0, self.evaluate(r));
                        result.set(x, y, 1, tile.at(x, y, 1));
                        result.set(x, y, 2, tile.at(x, y, 2));
                    }
                    CurveChannel::Green => {
                        result.set(x, y, 0, tile.at(x, y, 0));
                        let g = tile.at(x, y, 1);
                        result.set(x, y, 1, self.evaluate(g));
                        result.set(x, y, 2, tile.at(x, y, 2));
                    }
                    CurveChannel::Blue => {
                        result.set(x, y, 0, tile.at(x, y, 0));
                        result.set(x, y, 1, tile.at(x, y, 1));
                        let b = tile.at(x, y, 2);
                        result.set(x, y, 2, self.evaluate(b));
                    }
                    CurveChannel::All => {
                        for c in 0..3 {
                            let val = tile.at(x, y, c);
                            result.set(x, y, c, self.evaluate(val));
                        }
                    }
                    CurveChannel::Luminance => {
                        // Simplified: apply to green channel as luminance proxy
                        result.set(x, y, 0, tile.at(x, y, 0));
                        let val = tile.at(x, y, 1);
                        result.set(x, y, 1, self.evaluate(val));
                        result.set(x, y, 2, tile.at(x, y, 2));
                    }
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_curve_identity() {
        let curve = CurvesFilter::new(CurveChannel::All);
        assert!((curve.evaluate(0.0) - 0.0).abs() < 0.01);
        assert!((curve.evaluate(0.5) - 0.5).abs() < 0.01);
        assert!((curve.evaluate(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn inverse_curve() {
        let mut curve = CurvesFilter::new(CurveChannel::All);
        curve.add_point(0.0, 1.0).unwrap();
        curve.add_point(1.0, 0.0).unwrap();

        assert!((curve.evaluate(0.0) - 1.0).abs() < 0.01);
        assert!((curve.evaluate(0.5) - 0.5).abs() < 0.01);
        assert!((curve.evaluate(1.0) - 0.0).abs() < 0.01);
    }

    #[test]
    fn s_curve_contrast() {
        let mut curve = CurvesFilter::new(CurveChannel::All);
        curve.add_point(0.0, 0.0).unwrap();
        curve.add_point(0.25, 0.1).unwrap();
        curve.add_point(0.5, 0.5).unwrap();
        curve.add_point(0.75, 0.9).unwrap();
        curve.add_point(1.0, 1.0).unwrap();

        // S-curve should increase contrast
        assert!(curve.evaluate(0.25) < 0.25); // Darken shadows
        assert!(curve.evaluate(0.75) > 0.75); // Brighten highlights
    }

    #[test]
    fn custom_curve_points() {
        let mut curve = CurvesFilter::new(CurveChannel::All);
        curve.add_point(0.0, 0.0).unwrap();
        curve.add_point(0.5, 0.7).unwrap();
        curve.add_point(1.0, 1.0).unwrap();

        // Mid-tones should be brightened
        assert!(curve.evaluate(0.5) > 0.5);
    }

    #[test]
    fn clamping() {
        let curve = CurvesFilter::new(CurveChannel::All);
        assert!(curve.evaluate(-0.5) >= 0.0);
        assert!(curve.evaluate(1.5) <= 1.0);
    }

    #[test]
    fn invalid_control_point() {
        let mut curve = CurvesFilter::new(CurveChannel::All);
        assert!(curve.add_point(-0.1, 0.5).is_err());
        assert!(curve.add_point(0.5, 1.5).is_err());
    }
}
