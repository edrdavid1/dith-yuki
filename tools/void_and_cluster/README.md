# Void-and-cluster blue-noise generator

Produces `assets/blue_noise_128_u16.{png,raw,sha256,generator.txt}` for the
`blue_noise` dither family (`ED-on-GPU.md` §3.2.1).

Implements Ulichney's void-and-cluster method (Proc. SPIE 1913, 1993).

## Regenerating

```bash
cd tools/void_and_cluster
python3 -m venv .venv
. .venv/bin/activate
pip install -r requirements.txt

python generate.py
python verify.py   # permutation + spectrum sanity check
```

Expected runtime: ~10–60 seconds for 128×128.

Do not commit `.venv/`.

## Outputs

| File | Role |
|------|------|
| `blue_noise_128_u16.raw` | Canonical LE u16 ranks (embedded in Rust) |
| `blue_noise_128_u16.png` | Inspectable visualization |
| `blue_noise_128_u16.sha256` | SHA-256 of the `.raw` bytes |
| `blue_noise_128_u16.raw.sha256` | Same digest under an explicit raw name |
| `blue_noise_128_u16.generator.txt` | `size`, `sigma`, `seed`, algorithm |

Parameters (committed recipe): `size=128`, `sigma=1.5`, `seed=0x5EED`.
