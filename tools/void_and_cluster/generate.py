#!/usr/bin/env python3
"""Generate blue-noise rank texture via Ulichney's void-and-cluster algorithm.

Output:
  assets/blue_noise_128_u16.png            -- 16-bit grayscale, ranks 0..16383
  assets/blue_noise_128_u16.raw            -- little-endian u16 ranks (embedded)
  assets/blue_noise_128_u16.sha256         -- sha256 of the raw bytes
  assets/blue_noise_128_u16.raw.sha256     -- same digest (explicit raw name)
  assets/blue_noise_128_u16.generator.txt  -- parameters

Reference: Ulichney, "The void-and-cluster method for dither array generation",
Proc. SPIE 1913, 1993.
"""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def gaussian_kernel_circular(size: int, sigma: float) -> np.ndarray:
    """Circular (tileable) 2D Gaussian, normalized to sum 1."""
    coords = np.arange(size)
    dist = np.minimum(coords, size - coords)  # wrap-around distance
    dy, dx = np.meshgrid(dist, dist, indexing="ij")
    d2 = dx * dx + dy * dy
    k = np.exp(-d2 / (2.0 * sigma * sigma))
    return k / k.sum()


def contribution(kernel: np.ndarray, x: int, y: int) -> np.ndarray:
    """Energy contribution of a pixel at (x, y): the kernel shifted so its
    origin lands on (x, y), with circular wraparound."""
    return np.roll(np.roll(kernel, y, axis=0), x, axis=1)


def void_and_cluster(size: int, sigma: float, seed: int) -> np.ndarray:
    n = size * size
    half = n // 2
    rng = np.random.default_rng(seed)

    kernel = gaussian_kernel_circular(size, sigma)
    k_fft = np.fft.fft2(kernel)

    # --- Phase 0: initial 50% random pattern -----------------------------
    pattern = np.zeros((size, size), dtype=np.float64)
    idx = rng.permutation(n)[:half]
    ys, xs = np.unravel_index(idx, (size, size))
    pattern[ys, xs] = 1.0

    energy = np.real(np.fft.ifft2(np.fft.fft2(pattern) * k_fft))

    # --- Phase 0b: stabilize by cluster-removal / void-placement ---------
    for _ in range(100_000):
        masked = np.where(pattern == 1.0, energy, -np.inf)
        cy, cx = np.unravel_index(np.argmax(masked), pattern.shape)

        pattern[cy, cx] = 0.0
        energy -= contribution(kernel, cx, cy)

        masked = np.where(pattern == 0.0, energy, np.inf)
        vy, vx = np.unravel_index(np.argmin(masked), pattern.shape)

        if (vy, vx) == (cy, cx):
            pattern[cy, cx] = 1.0
            energy += contribution(kernel, cx, cy)
            break

        pattern[vy, vx] = 1.0
        energy += contribution(kernel, vx, vy)
    else:
        raise RuntimeError("stabilization did not converge")

    initial_pattern = pattern.copy()

    # --- Phase 1: ranks half-1 .. 0 --------------------------------------
    rank = np.full((size, size), -1, dtype=np.int32)
    for r in range(half - 1, -1, -1):
        masked = np.where(pattern == 1.0, energy, -np.inf)
        cy, cx = np.unravel_index(np.argmax(masked), pattern.shape)
        pattern[cy, cx] = 0.0
        energy -= contribution(kernel, cx, cy)
        rank[cy, cx] = r

    # --- Phase 2: ranks half .. N-1 --------------------------------------
    pattern = initial_pattern.copy()
    energy = np.real(np.fft.ifft2(np.fft.fft2(pattern) * k_fft))
    for r in range(half, n):
        masked = np.where(pattern == 0.0, energy, np.inf)
        vy, vx = np.unravel_index(np.argmin(masked), pattern.shape)
        pattern[vy, vx] = 1.0
        energy += contribution(kernel, vx, vy)
        rank[vy, vx] = r

    # Sanity: rank is a permutation of 0..N-1
    assert np.array_equal(np.sort(rank.flatten()), np.arange(n)), "rank is not a permutation"
    return rank


def main() -> int:
    size = 128
    sigma = 1.5
    seed = 0x5EED

    rank = void_and_cluster(size=size, sigma=sigma, seed=seed)

    out_dir = Path(__file__).resolve().parent.parent.parent / "assets"
    out_dir.mkdir(parents=True, exist_ok=True)

    out_png = out_dir / "blue_noise_128_u16.png"
    out_raw = out_dir / "blue_noise_128_u16.raw"
    # uint16 ndarray → 16-bit grayscale PNG (Pillow picks I;16 from dtype).
    Image.fromarray(rank.astype(np.uint16)).save(out_png, optimize=False, pnginfo=None)

    raw_bytes = rank.astype("<u2").tobytes()  # little-endian u16
    out_raw.write_bytes(raw_bytes)

    digest = hashlib.sha256(raw_bytes).hexdigest()
    sha_line = f"{digest}  blue_noise_128_u16.raw\n"
    (out_dir / "blue_noise_128_u16.sha256").write_text(sha_line)
    (out_dir / "blue_noise_128_u16.raw.sha256").write_text(sha_line)
    (out_dir / "blue_noise_128_u16.generator.txt").write_text(
        f"size={size}\n"
        f"sigma={sigma}\n"
        f"seed=0x{seed:X}\n"
        f"algorithm=void-and-cluster (Ulichney 1993)\n"
        f"rank_range=0..{size * size - 1}\n"
        f"sha256={digest}\n"
        f"canonical=blue_noise_128_u16.raw (little-endian u16)\n"
        f"png=blue_noise_128_u16.png (inspectable visualization)\n"
    )

    print(f"wrote {out_png}")
    print(f"wrote {out_raw}")
    print(f"sha256 {digest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
