//! Named palette builders for the parity matrix.

pub fn palette_n(n: usize) -> Vec<[u8; 4]> {
    match n {
        0 => vec![],
        1 => vec![[128, 128, 128, 255]],
        2 => vec![[0, 0, 0, 255], [255, 255, 255, 255]],
        _ => {
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                let t = if n == 1 {
                    0u32
                } else {
                    (i as u32 * 255) / (n as u32 - 1)
                };
                let r = t as u8;
                let g = ((t.wrapping_mul(3)) % 256) as u8;
                let b = ((t.wrapping_mul(7)) % 256) as u8;
                v.push([r, g, b, 255]);
            }
            v
        }
    }
}

pub fn palette_two_same() -> Vec<[u8; 4]> {
    vec![[100, 100, 100, 255], [100, 100, 100, 255]]
}

pub fn palette_two_close() -> Vec<[u8; 4]> {
    vec![[100, 100, 100, 255], [101, 100, 100, 255]]
}

pub fn palette_cga4() -> Vec<[u8; 4]> {
    vec![
        [0, 0, 0, 255],
        [0, 170, 170, 255],
        [170, 0, 170, 255],
        [170, 170, 170, 255],
    ]
}

pub fn palette_unsorted() -> Vec<[u8; 4]> {
    vec![
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [200, 50, 50, 255],
        [50, 200, 50, 255],
        [50, 50, 200, 255],
        [128, 128, 128, 255],
    ]
}

pub fn palette_gray8() -> Vec<[u8; 4]> {
    (0..8)
        .map(|i| {
            let g = (i * 255 / 7) as u8;
            [g, g, g, 255]
        })
        .collect()
}

pub fn palette_primaries() -> Vec<[u8; 4]> {
    vec![
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [0, 255, 255, 255],
        [255, 0, 255, 255],
        [255, 255, 0, 255],
        [0, 0, 0, 255],
        [255, 255, 255, 255],
    ]
}

/// Two palette colors equidistant from mid-gray → tie broken by lower index.
pub fn palette_tie() -> Vec<[u8; 4]> {
    // Distance from (128,128,128) to (128-d,128,128) equals (128+d,128,128).
    vec![
        [100, 128, 128, 255], // index 0 wins on tie with equal distance
        [156, 128, 128, 255],
        [0, 0, 0, 255],
        [255, 255, 255, 255],
    ]
}

pub fn palette_out_of_gamut() -> Vec<[u8; 4]> {
    vec![[100, 100, 100, 255], [150, 150, 150, 255]]
}

#[derive(Clone, Copy)]
pub struct NamedPalette {
    pub tag: &'static str,
    pub colors: fn() -> Vec<[u8; 4]>,
}

pub fn quick_palettes() -> Vec<NamedPalette> {
    vec![
        NamedPalette {
            tag: "p1",
            colors: || palette_n(1),
        },
        NamedPalette {
            tag: "p2",
            colors: || palette_n(2),
        },
        NamedPalette {
            tag: "p4",
            colors: || palette_n(4),
        },
        NamedPalette {
            tag: "p16",
            colors: || palette_n(16),
        },
        NamedPalette {
            tag: "p256",
            colors: || palette_n(256),
        },
        // Critical divergence classes (must appear in quick inventory).
        NamedPalette {
            tag: "p2same",
            colors: palette_two_same,
        },
        NamedPalette {
            tag: "ptie",
            colors: palette_tie,
        },
        NamedPalette {
            tag: "pog",
            colors: palette_out_of_gamut,
        },
    ]
}

pub fn full_palettes() -> Vec<NamedPalette> {
    let mut v = quick_palettes();
    v.extend([
        NamedPalette {
            tag: "p2same",
            colors: palette_two_same,
        },
        NamedPalette {
            tag: "p2close",
            colors: palette_two_close,
        },
        NamedPalette {
            tag: "p3",
            colors: || palette_n(3),
        },
        NamedPalette {
            tag: "pcga",
            colors: palette_cga4,
        },
        NamedPalette {
            tag: "p8",
            colors: || palette_n(8),
        },
        NamedPalette {
            tag: "p64",
            colors: || palette_n(64),
        },
        NamedPalette {
            tag: "p255",
            colors: || palette_n(255),
        },
        NamedPalette {
            tag: "p257",
            colors: || palette_n(257),
        },
        NamedPalette {
            tag: "punsorted",
            colors: palette_unsorted,
        },
        NamedPalette {
            tag: "pgray",
            colors: palette_gray8,
        },
        NamedPalette {
            tag: "pprim",
            colors: palette_primaries,
        },
        NamedPalette {
            tag: "ptie",
            colors: palette_tie,
        },
        NamedPalette {
            tag: "pog",
            colors: palette_out_of_gamut,
        },
    ]);
    v
}
