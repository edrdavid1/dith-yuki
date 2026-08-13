# Production-release tracks H–P + C4.1

Раскладка [ROADMAP_production_release.md](./ROADMAP_production_release.md) и
[ADDENDUM_release_plan_L_C4.md](./ADDENDUM_release_plan_L_C4.md) на спеки в
том же формате, что A–G (`requirements` / `design` / `tasks`). Track N —
отдельный бриф [TASK_track_n_undo_redo.md](./TASK_track_n_undo_redo.md).
Бета (dirty / Guard / Apply-replace / QA): [track-p-beta/](./track-p-beta/).

**Буквы не переоткрывать.** Track G в репо — Welcome
([track-g-welcome/](./track-g-welcome/)). ED-ядра из ROADMAP «Track G»
здесь — **Track M**. C4 v1 уже закрыт в [track-c-phase1-filters/](./track-c-phase1-filters/);
аддендум C4 — follow-up **C4.1**.

Вес chroma/contrast при extract — не отдельный трек: [color-lab.md](./color-lab.md) задача 6.

## Карта

| ID | Папка | Источник | Gate | As-built |
|----|--------|----------|------|----------|
| **K** | [track-k-slider/](./track-k-slider/) | ROADMAP K | нет | закрыто 2026-08-13: Slider + NumberInput; debounce только в `useEffectLayer`; editors без сырых number/range |
| **J** | [track-j-glitch/](./track-j-glitch/) | ROADMAP J | нет | закрыто 2026-08-13: GlobalCoord + HALO cap + seam tests |
| **H** | [track-h-bayer-params/](./track-h-bayer-params/) | ROADMAP H | Bias — сразу; Angle — после A2 (уже зелёный) | `threshold_bias` / `pattern_angle` on `DitherParamsV2`; Block_Then_Rotate; GPU skip when non-default |
| **L** | [track-l-oklab-volume/](./track-l-oklab-volume/) | ADDENDUM L | нет | закрыто 2026-08-13: `colors_to_oklab` / `get_palette_oklab`; static gamut JSON; `PaletteVolumeViewer` (three, L up) |
| **C4.1** | [track-c4-svg-followup/](./track-c4-svg-followup/) | ADDENDUM C4 | нет | `engine-io::svg_export` v1; holes out of scope; UI всегда GreedyMeshing |
| **M** | [track-m-ed-kernels/](./track-m-ed-kernels/) | ROADMAP G (remap) | A1 закрыт (уже); Serpentine отдельным шагом после ядер | V2: только FS/Atkinson; legacy `DiffusionKernel` имеет JJN/Stucki |
| **I** | [track-i-filter-blend/](./track-i-filter-blend/) | ROADMAP I | A1 закрыт (уже) | `FilterInstance.opacity` + `blend_mode`; wrapper `apply_filter_with_blend` + `blend_tile`; UI на EffectSettingsPanel; DnD = `reorder_filter` |
| Color Lab §6 | [color-lab.md](./color-lab.md) | ROADMAP (не трек) | нет | `generate.rs` без chroma/contrast весов |
| **N** | [track-n-undo-redo/](./track-n-undo-redo/) | [TASK_track_n_undo_redo.md](./TASK_track_n_undo_redo.md) | K закрыт (debounce) | snapshot `Arc<Document>` depth 50; wrapper; orphan `evict_layer`; Edit + ⌘Z |
| **O** | [track-o-updates/](./track-o-updates/) | бета: in-app updates | нет (dirty-flag смягчает Restart_Guard, не блокирует) | нет плагина; Help пустой; `0.1.0`; GitHub `edrdavid1/dith-yuki` |
| **P** | [track-p-beta/](./track-p-beta/) | бета: dirty, Guard, Apply-replace, QA | N закрыт (уже) | нет `unsaved`; Apply = всегда `add_palette`; A §6.2 / D §5.3 пустые |

## Параллельность (из аддендума, буквы актуальные)

```text
Параллельно, без ограничений между собой:
  K  J  H§Bias  L  C4.1  Color Lab §6

После A1 (уже закрыт) — можно сразу, но не раньше ядер для Serpentine:
  M1 ED kernels → M2 Serpentine отдельным шагом
  I  per-filter Opacity/Blend (обёртка в диспетчере)

После A2 (уже закрыт):
  H§Angle

После C + A (уже закрыты):
  D GPU — без изменений, см. track-d-gpu/

После K (уже закрыт) — можно сразу, независимо от H–M:
  N  Undo/Redo (snapshot history)

Независимо от H–N / C4.1 (бета-канал):
  O  In-app updates (Tauri plugin + GitHub latest.json)

Бета-гейт (продукт, не движок) — параллельно C4.1 и Color Lab §6:
  P1 Dirty + Unsaved_Guard   (до O3, чтобы Guard умел skip if clean)
  P2 Color Lab Apply replace (до P3)
  P4 Ручной QA A §6.2 / D §5.3
Beta 0 = P1+P2+P4 + C4.1 + Color Lab §6
Beta 1 = Beta 0 + P3 Import Layer + O
```

Важность «perceptual vs retro» из аддендума — про распределение людей,
не про отмену гейтов внутри трека.
