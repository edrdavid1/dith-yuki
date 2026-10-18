# Z3 cold ≥12 MPix TIME_TO_FRESH (dev)

Doc: 4000×3000 (12 MPix), Floyd–Steinberg, cold Processed/Composite.
Sequence: zoom-in → apply → mid-pass zooms → final zoom 0.25 + full L0 prefix.
`time_to_fresh_ms` = wall until L0 Processed done==total (ED pass).
`composite_holes` = visible Composite still missing at that moment.

| profile | visible | composite_holes | l0_done/total | time_to_fresh_ms | wall_ms |
| --- | ---: | ---: | --- | ---: | ---: |
| dev | 12 | 12 | 192/192 | 752.0 | 752.0 |
