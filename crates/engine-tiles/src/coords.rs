//! Единый источник истины для перевода локальных координат пикселя внутри
//! тайла в глобальные координаты документа. Используется всеми фильтрами,
//! которым нужен непрерывный по всему холсту паттерн (dithering, CRT,
//! halftone, mega-pixel grid).
//!
//! ВАЖНО: не дублировать формулу `tile.x * TILE_SIZE + local_x` в фильтрах.
//! Всегда конструировать координату через [`GlobalCoord::from_local`].

use crate::{TileCoord, TILE_SIZE};

/// Глобальная пиксельная координата документа (не тайла).
///
/// Хранит координату как `u32` — подходит для основного потока (core area
/// тайла без учёта halo). Для фильтров, работающих с halo-регионом и
/// потенциально отрицательными координатами, используйте [`GlobalCoordSigned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalCoord {
    pub x: u32,
    pub y: u32,
}

impl GlobalCoord {
    /// Перевод локальной координаты (core area) внутри тайла в глобальную
    /// координату документа.
    ///
    /// `local_x` и `local_y` должны быть в `[0, TILE_SIZE)`.
    #[inline]
    pub fn from_local(tile: TileCoord, local_x: u32, local_y: u32) -> Self {
        debug_assert!(local_x < TILE_SIZE, "local_x out of tile bounds");
        debug_assert!(local_y < TILE_SIZE, "local_y out of tile bounds");
        Self {
            x: tile.x * TILE_SIZE + local_x,
            y: tile.y * TILE_SIZE + local_y,
        }
    }

    /// Перевод произвольной пиксельной координаты внутри тайла (включая halo)
    /// в глобальную координату без смещения на halo.
    ///
    /// Используется в фильтрах, которые итерируют по полному тайлу (с halo)
    /// но не нуждаются в halo-коррекции для паттерна (legacy ordered dither).
    /// Для фильтров, корректно учитывающих halo, используйте [`GlobalCoordSigned::from_local_with_halo`].
    #[inline]
    pub fn from_tile_pixel(tile: TileCoord, pixel_x: u32, pixel_y: u32) -> Self {
        Self {
            x: tile.x * TILE_SIZE + pixel_x,
            y: tile.y * TILE_SIZE + pixel_y,
        }
    }

    /// Выравнивание координаты по сетке супер-пикселей (mega-pixel grid).
    ///
    /// `pixel_size = 1` эквивалентно отсутствию выравнивания — вызывать
    /// безусловно, не оборачивать в `if pixel_size > 1`.
    #[inline]
    #[must_use]
    pub fn aligned(self, pixel_size: u32) -> Self {
        debug_assert!(pixel_size >= 1, "pixel_size must be >= 1");
        Self {
            x: (self.x / pixel_size) * pixel_size,
            y: (self.y / pixel_size) * pixel_size,
        }
    }

    /// Координата внутри повторяющейся ячейки паттерна заданного размера
    /// (напр. Bayer 8×8 → `pattern_size = 8`). Удобно для индексации в
    /// пороговые матрицы.
    #[inline]
    #[must_use]
    pub fn pattern_cell(self, pattern_size: u32) -> (u32, u32) {
        debug_assert!(pattern_size >= 1, "pattern_size must be >= 1");
        (self.x % pattern_size, self.y % pattern_size)
    }
}

/// Глобальная пиксельная координата со знаком — для фильтров, работающих с
/// halo-регионом, где координата может стать отрицательной (тайл на краю
/// документа, halo выходит за пределы).
///
/// Основное использование: ordered dithering и error diffusion с `pixel_size > 1`,
/// где halo-пиксели нужно адресовать в глобальном пространстве.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalCoordSigned {
    pub x: i32,
    pub y: i32,
}

impl GlobalCoordSigned {
    /// Перевод локальной координаты (включая halo) в глобальную координату.
    ///
    /// `local_x/local_y` — координата в пределах полного тайла (с halo),
    /// т.е. `[0, TILE_SIZE + 2*HALO)`. Halo-offset вычитается автоматически.
    #[inline]
    pub fn from_local_with_halo(tile: TileCoord, local_x: u32, local_y: u32, halo: u32) -> Self {
        Self {
            x: tile.x as i32 * TILE_SIZE as i32 + local_x as i32 - halo as i32,
            y: tile.y as i32 * TILE_SIZE as i32 + local_y as i32 - halo as i32,
        }
    }

    /// Выравнивание по сетке супер-пикселей (mega-pixel grid) с использованием
    /// `div_euclid` для корректной работы с отрицательными координатами.
    #[inline]
    #[must_use]
    pub fn aligned(self, pixel_size: u32) -> Self {
        debug_assert!(pixel_size >= 1, "pixel_size must be >= 1");
        let ps = pixel_size as i32;
        Self {
            x: self.x.div_euclid(ps) * ps,
            y: self.y.div_euclid(ps) * ps,
        }
    }

    /// Координата внутри повторяющейся ячейки паттерна. Использует `rem_euclid`
    /// для корректного поведения при отрицательных координатах.
    #[inline]
    #[must_use]
    pub fn pattern_cell(self, pattern_size: u32) -> (u32, u32) {
        debug_assert!(pattern_size >= 1, "pattern_size must be >= 1");
        let ps = pattern_size as i32;
        (self.x.rem_euclid(ps) as u32, self.y.rem_euclid(ps) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(x: u32, y: u32) -> TileCoord {
        TileCoord { level: 0, x, y }
    }

    // ─── GlobalCoord tests ───────────────────────────────────────────────────

    #[test]
    fn from_local_origin() {
        let g = GlobalCoord::from_local(tile(0, 0), 0, 0);
        assert_eq!(g, GlobalCoord { x: 0, y: 0 });
    }

    #[test]
    fn from_local_second_tile() {
        let g = GlobalCoord::from_local(tile(1, 0), 0, 0);
        assert_eq!(g, GlobalCoord { x: TILE_SIZE, y: 0 });
    }

    /// Ключевой тест: паттерн должен быть непрерывным на стыке тайлов.
    /// Последний пиксель тайла (0,0) и первый пиксель тайла (1,0) должны
    /// давать соседние ячейки паттерна, а не обе стартовать с нуля.
    #[test]
    fn pattern_continuous_across_tile_boundary() {
        let last_of_tile0 = GlobalCoord::from_local(tile(0, 0), TILE_SIZE - 1, 0);
        let first_of_tile1 = GlobalCoord::from_local(tile(1, 0), 0, 0);

        let (px0, _) = last_of_tile0.pattern_cell(8);
        let (px1, _) = first_of_tile1.pattern_cell(8);

        // TILE_SIZE=256 кратен 8, поэтому 255 % 8 = 7, а 256 % 8 = 0.
        assert_eq!(px0, 7);
        assert_eq!(px1, 0);
    }

    #[test]
    fn aligned_pixel_size_one_is_noop() {
        let g = GlobalCoord { x: 137, y: 42 };
        assert_eq!(g.aligned(1), g);
    }

    #[test]
    fn aligned_snaps_down_to_grid() {
        let g = GlobalCoord { x: 137, y: 42 };
        let a = g.aligned(4);
        assert_eq!(a, GlobalCoord { x: 136, y: 40 });
    }

    #[test]
    fn aligned_grid_has_no_seam_at_tile_boundary() {
        // pixel_size=4: 256 кратно 4, швов быть не должно
        let last_of_tile0 = GlobalCoord::from_local(tile(0, 0), TILE_SIZE - 1, 0).aligned(4);
        let first_of_tile1 = GlobalCoord::from_local(tile(1, 0), 0, 0).aligned(4);
        // 255 -> 252, 256 -> 256, разница = 4
        assert_eq!(first_of_tile1.x - last_of_tile0.x, 4);
    }

    // ─── GlobalCoordSigned tests ─────────────────────────────────────────────

    #[test]
    fn signed_from_local_with_halo_origin() {
        let g = GlobalCoordSigned::from_local_with_halo(tile(0, 0), 0, 0, 2);
        // halo=2, local (0,0) -> global (-2, -2)
        assert_eq!(g, GlobalCoordSigned { x: -2, y: -2 });
    }

    #[test]
    fn signed_from_local_with_halo_core_start() {
        let g = GlobalCoordSigned::from_local_with_halo(tile(0, 0), 2, 2, 2);
        // halo=2, local (2,2) -> global (0, 0) = start of core area
        assert_eq!(g, GlobalCoordSigned { x: 0, y: 0 });
    }

    #[test]
    fn signed_from_local_with_halo_second_tile() {
        let g = GlobalCoordSigned::from_local_with_halo(tile(1, 0), 2, 2, 2);
        assert_eq!(g, GlobalCoordSigned { x: TILE_SIZE as i32, y: 0 });
    }

    #[test]
    fn signed_aligned_positive() {
        let g = GlobalCoordSigned { x: 137, y: 42 };
        let a = g.aligned(4);
        assert_eq!(a, GlobalCoordSigned { x: 136, y: 40 });
    }

    #[test]
    fn signed_aligned_negative() {
        // div_euclid: -1 / 4 = -1, so -1 * 4 = -4
        let g = GlobalCoordSigned { x: -1, y: -3 };
        let a = g.aligned(4);
        assert_eq!(a, GlobalCoordSigned { x: -4, y: -4 });
    }

    #[test]
    fn signed_pattern_cell_positive() {
        let g = GlobalCoordSigned { x: 255, y: 0 };
        assert_eq!(g.pattern_cell(8), (7, 0));
    }

    #[test]
    fn signed_pattern_cell_negative() {
        // rem_euclid: -1 % 8 = 7
        let g = GlobalCoordSigned { x: -1, y: -1 };
        assert_eq!(g.pattern_cell(8), (7, 7));
    }

    #[test]
    fn signed_pattern_continuous_across_tile_boundary() {
        let halo = 2u32;
        // Last pixel of core area in tile(0,0): local_x = halo + TILE_SIZE - 1
        let last = GlobalCoordSigned::from_local_with_halo(tile(0, 0), halo + TILE_SIZE - 1, halo, halo);
        // First pixel of core area in tile(1,0): local_x = halo
        let first = GlobalCoordSigned::from_local_with_halo(tile(1, 0), halo, halo, halo);

        let (px_last, _) = last.pattern_cell(8);
        let (px_first, _) = first.pattern_cell(8);

        assert_eq!(px_last, 7); // 255 % 8 = 7
        assert_eq!(px_first, 0); // 256 % 8 = 0
    }

    /// Test that GlobalCoordSigned handles the halo region (negative global coords)
    /// correctly — pattern_cell must give valid non-negative indices even for halo pixels
    /// at tile (0,0) where global coords become negative.
    #[test]
    fn signed_halo_negative_coords_give_valid_pattern_cell() {
        let halo = 2u32;
        // Pixel at local (0, 0) with halo=2 on tile (0,0) → global (-2, -2)
        let g = GlobalCoordSigned::from_local_with_halo(tile(0, 0), 0, 0, halo);
        assert_eq!(g, GlobalCoordSigned { x: -2, y: -2 });

        // pattern_cell should use rem_euclid, giving valid indices
        let (px, py) = g.pattern_cell(8);
        assert_eq!(px, 6); // -2 rem_euclid 8 = 6
        assert_eq!(py, 6);
    }
}
