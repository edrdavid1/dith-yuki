# Z3 cold ≥12 MPix TIME_TO_FRESH (dev)

Doc: 4000×3000 (12 MPix), Floyd–Steinberg, cold Processed/Composite.
Sequence: zoom-in → apply → mid-pass zooms → final zoom 0.25 + full L0 prefix.
Clock starts at final zoom (after mid-pass work already ran).
`time_to_l0_ms` = L0 Processed done==total; `time_to_fresh_ms` = visible Composite holes=0.

| profile | visible | composite_holes | l0_done/total | time_to_l0_ms | time_to_fresh_ms | wall_ms |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| dev | 12 | 0 | 192/192 | 749.6 | 909.2 | 909.2 |
