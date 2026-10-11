#!/usr/bin/env python3
"""Sanity-check the committed void-and-cluster blue-noise texture."""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent.parent
ASSETS = ROOT / "assets"
RAW = ASSETS / "blue_noise_128_u16.raw"
PNG = ASSETS / "blue_noise_128_u16.png"
SHA = ASSETS / "blue_noise_128_u16.sha256"


def main() -> int:
    raw = RAW.read_bytes()
    assert len(raw) == 128 * 128 * 2, f"raw size {len(raw)}"
    arr = np.frombuffer(raw, dtype="<u2").reshape(128, 128)
    print("raw shape:", arr.shape, "dtype:", arr.dtype, "min:", arr.min(), "max:", arr.max())

    flat = np.sort(arr.flatten())
    assert np.array_equal(flat, np.arange(16384)), "not a permutation"
    print("permutation: OK")

    digest = hashlib.sha256(raw).hexdigest()
    expected = SHA.read_text().split()[0]
    assert digest == expected, f"sha mismatch: {digest} != {expected}"
    print("sha256: OK")

    img = Image.open(PNG)
    png_arr = np.array(img)
    print("png shape:", png_arr.shape, "dtype:", png_arr.dtype)
    assert png_arr.shape == (128, 128)

    f = np.fft.fftshift(np.fft.fft2(arr.astype(np.float64) - arr.mean()))
    power = np.abs(f) ** 2
    power[64, 64] = 0  # zero out DC
    top = np.unravel_index(np.argsort(power.flatten())[-5:], power.shape)
    peaks = list(zip(*top))
    print("top-5 non-DC peaks:", peaks)
    # Blue noise should not concentrate all top peaks at one frequency bin.
    unique = {(int(y), int(x)) for y, x in peaks}
    assert len(unique) >= 3, f"peaks too concentrated: {unique}"
    print("spectrum spread: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
