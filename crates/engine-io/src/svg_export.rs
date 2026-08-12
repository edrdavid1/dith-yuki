//! SVG export via greedy meshing and contour tracing.
//!
//! Converts an RGBA8 raster into an SVG string of merged `<rect>` elements
//! (greedy meshing) or external `<path>` contours.

use crate::sandbox::{resolve_export_path, SandboxError};
use std::fs;
use thiserror::Error;

/// SVG vectorization algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAlgorithm {
    GreedyMeshing,
    /// External contours only (holes out of scope for v1).
    ContourTracing,
}

/// Options for [`raster_to_svg`].
#[derive(Debug, Clone)]
pub struct SvgExportOptions {
    pub algorithm: SvgAlgorithm,
    /// Per-channel absolute delta for merging similar colors (0 = exact).
    pub tolerance: u8,
}

impl Default for SvgExportOptions {
    fn default() -> Self {
        Self {
            algorithm: SvgAlgorithm::GreedyMeshing,
            tolerance: 0,
        }
    }
}

/// Errors from SVG export.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SvgExportError {
    #[error("rgba buffer length mismatch: expected {expected}, got {got}")]
    BufferLen { expected: usize, got: usize },
    #[error("sandbox: {0}")]
    Sandbox(#[from] SandboxError),
    #[error("io: {0}")]
    Io(String),
}

/// Convert an RGBA8 raster to an SVG document string.
pub fn raster_to_svg(
    width: u32,
    height: u32,
    rgba: &[u8],
    opts: &SvgExportOptions,
) -> Result<String, SvgExportError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .unwrap_or(usize::MAX);
    if rgba.len() != expected {
        return Err(SvgExportError::BufferLen {
            expected,
            got: rgba.len(),
        });
    }

    let body = match opts.algorithm {
        SvgAlgorithm::GreedyMeshing => greedy_mesh_rects(width, height, rgba, opts.tolerance),
        SvgAlgorithm::ContourTracing => contour_paths(width, height, rgba, opts.tolerance),
    };

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
{body}</svg>
"#,
        w = width,
        h = height,
        body = body
    ))
}

/// Validate path via sandbox and write SVG to disk.
pub fn write_svg_file(
    path: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
    opts: &SvgExportOptions,
) -> Result<(), SvgExportError> {
    let svg = raster_to_svg(width, height, rgba, opts)?;
    let out = resolve_export_path(path, &["svg"])?;
    fs::write(&out, svg).map_err(|e| SvgExportError::Io(e.to_string()))?;
    Ok(())
}

#[inline]
fn color_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[inline]
fn colors_match(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.abs_diff(*y) <= tol)
}

fn hex_rgb(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Greedy meshing: grow maximal width then height rectangles of equal color.
fn greedy_mesh_rects(width: u32, height: u32, rgba: &[u8], tol: u8) -> String {
    let w = width as usize;
    let h = height as usize;
    let mut visited = vec![false; w * h];
    let mut out = String::new();

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if visited[idx] {
                continue;
            }
            let color = color_at(rgba, width, x, y);
            // Skip fully transparent
            if color[3] == 0 {
                visited[idx] = true;
                continue;
            }

            // Grow width
            let mut rw = 1u32;
            while x + rw < width {
                let nx = x + rw;
                let nidx = (y * width + nx) as usize;
                if visited[nidx] || !colors_match(color_at(rgba, width, nx, y), color, tol) {
                    break;
                }
                rw += 1;
            }

            // Grow height while full row matches
            let mut rh = 1u32;
            'grow: while y + rh < height {
                for dx in 0..rw {
                    let nx = x + dx;
                    let ny = y + rh;
                    let nidx = (ny * width + nx) as usize;
                    if visited[nidx] || !colors_match(color_at(rgba, width, nx, ny), color, tol) {
                        break 'grow;
                    }
                }
                rh += 1;
            }

            for dy in 0..rh {
                for dx in 0..rw {
                    visited[((y + dy) * width + (x + dx)) as usize] = true;
                }
            }

            if color[3] == 255 {
                out.push_str(&format!(
                    r#"  <rect x="{x}" y="{y}" width="{rw}" height="{rh}" fill="{fill}"/>
"#,
                    fill = hex_rgb(color)
                ));
            } else {
                out.push_str(&format!(
                    r#"  <rect x="{x}" y="{y}" width="{rw}" height="{rh}" fill="{fill}" fill-opacity="{a:.4}"/>
"#,
                    fill = hex_rgb(color),
                    a = color[3] as f32 / 255.0
                ));
            }
        }
    }
    out
}

/// External contour tracing (Moore neighborhood) — holes out of scope for v1.
fn contour_paths(width: u32, height: u32, rgba: &[u8], tol: u8) -> String {
    let w = width as usize;
    let h = height as usize;
    let mut visited = vec![false; w * h];
    let mut out = String::new();

    // 8-connected Moore offsets clockwise starting from west of start
    const DIRS: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if visited[idx] {
                continue;
            }
            let color = color_at(rgba, width, x, y);
            if color[3] == 0 {
                visited[idx] = true;
                continue;
            }

            // Flood-fill component to mark visited, then emit bounding external path
            // via walking the outer edge of the component.
            let mut stack = vec![(x, y)];
            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            let mut cells = Vec::new();
            visited[idx] = true;
            while let Some((cx, cy)) = stack.pop() {
                cells.push((cx, cy));
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
                for (dx, dy) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let nx = nx as u32;
                    let ny = ny as u32;
                    let nidx = (ny * width + nx) as usize;
                    if visited[nidx] {
                        continue;
                    }
                    if colors_match(color_at(rgba, width, nx, ny), color, tol) {
                        visited[nidx] = true;
                        stack.push((nx, ny));
                    }
                }
            }

            // External contour: axis-aligned outline of the component bbox is wrong for
            // non-rect shapes. Walk boundary pixels with Moore for a closed path.
            let path = moore_external_path(&cells, width, height, &DIRS);
            if path.is_empty() {
                continue;
            }
            let mut d = String::new();
            for (i, (px, py)) in path.iter().enumerate() {
                if i == 0 {
                    d.push_str(&format!("M{} {} ", px, py));
                } else {
                    d.push_str(&format!("L{} {} ", px, py));
                }
            }
            d.push('Z');
            out.push_str(&format!(
                r#"  <path d="{d}" fill="{fill}"/>
"#,
                fill = hex_rgb(color)
            ));
            let _ = (min_x, min_y, max_x, max_y); // silence if unused in future
        }
    }
    out
}

fn moore_external_path(
    cells: &[(u32, u32)],
    width: u32,
    height: u32,
    dirs: &[(i32, i32); 8],
) -> Vec<(i32, i32)> {
    use std::collections::HashSet;
    let set: HashSet<(u32, u32)> = cells.iter().copied().collect();
    // Start at leftmost topmost cell
    let start = cells
        .iter()
        .copied()
        .min_by_key(|&(x, y)| (y, x))
        .unwrap_or((0, 0));

    // Find a neighbor direction that steps outside, then walk clockwise.
    let mut path = Vec::new();
    let (mut cx, mut cy) = (start.0 as i32, start.1 as i32);

    // Find first boundary step: from start, look for first empty neighbor clockwise from west
    let mut start_dir = None;
    for i in 0..8 {
        let d = (4 + i) % 8; // start looking west
        let nx = cx + dirs[d].0;
        let ny = cy + dirs[d].1;
        let inside = nx >= 0
            && ny >= 0
            && (nx as u32) < width
            && (ny as u32) < height
            && set.contains(&(nx as u32, ny as u32));
        if !inside {
            // next clockwise that is inside becomes first move
            for j in 1..8 {
                let d2 = (d + j) % 8;
                let nx2 = cx + dirs[d2].0;
                let ny2 = cy + dirs[d2].1;
                let inside2 = nx2 >= 0
                    && ny2 >= 0
                    && (nx2 as u32) < width
                    && (ny2 as u32) < height
                    && set.contains(&(nx2 as u32, ny2 as u32));
                if inside2 {
                    start_dir = Some(d2);
                    break;
                }
            }
            break;
        }
    }

    // Single-pixel or solid with no exterior walk via 8-neigh: emit unit square
    let Some(mut dir) = start_dir else {
        return vec![
            (cx, cy),
            (cx + 1, cy),
            (cx + 1, cy + 1),
            (cx, cy + 1),
        ];
    };

    path.push((cx, cy));
    let (sx, sy) = (cx, cy);
    let start_out_dir = dir;
    // Walk until we return to start with same direction
    for _ in 0..(cells.len() * 8 + 8) {
        // Move
        cx += dirs[dir].0;
        cy += dirs[dir].1;
        path.push((cx, cy));
        let back_dir = (dir + 4) % 8;
        // From back_dir - 1 (prefer left), find next inside neighbor
        let mut found = None;
        for j in 0..8 {
            let d = (back_dir + 7 + j) % 8; // start one step CCW from back (= left-hand rule clockwise boundary)
            let nx = cx + dirs[d].0;
            let ny = cy + dirs[d].1;
            let inside = nx >= 0
                && ny >= 0
                && (nx as u32) < width
                && (ny as u32) < height
                && set.contains(&(nx as u32, ny as u32));
            if inside {
                found = Some(d);
                break;
            }
        }
        let Some(nd) = found else { break };
        dir = nd;
        if cx == sx && cy == sy && dir == start_out_dir {
            break;
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_field_one_rect() {
        let w = 4u32;
        let h = 4u32;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[255, 0, 0, 255]);
        }
        let svg = raster_to_svg(
            w,
            h,
            &rgba,
            &SvgExportOptions {
                algorithm: SvgAlgorithm::GreedyMeshing,
                tolerance: 0,
            },
        )
        .unwrap();
        assert!(svg.contains(r#"viewBox="0 0 4 4""#));
        let rects = svg.matches("<rect ").count();
        assert_eq!(rects, 1, "solid should be 1 rect, got:\n{svg}");
        assert!(svg.contains(r#"width="4""#));
        assert!(svg.contains(r#"height="4""#));
        assert!(svg.contains("#ff0000"));
    }

    #[test]
    fn checker_2x2_four_rects() {
        let w = 2u32;
        let h = 2u32;
        // black, white / white, black
        let rgba = vec![
            0, 0, 0, 255, 255, 255, 255, 255, //
            255, 255, 255, 255, 0, 0, 0, 255,
        ];
        let svg = raster_to_svg(
            w,
            h,
            &rgba,
            &SvgExportOptions {
                algorithm: SvgAlgorithm::GreedyMeshing,
                tolerance: 0,
            },
        )
        .unwrap();
        assert_eq!(svg.matches("<rect ").count(), 4);
    }

    #[test]
    fn contour_emits_path() {
        let w = 3u32;
        let h = 3u32;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        // single center pixel opaque
        let i = ((1 * w + 1) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&[0, 128, 255, 255]);
        let svg = raster_to_svg(
            w,
            h,
            &rgba,
            &SvgExportOptions {
                algorithm: SvgAlgorithm::ContourTracing,
                tolerance: 0,
            },
        )
        .unwrap();
        assert!(svg.contains("<path "));
        assert!(svg.contains("#0080ff"));
    }
}
