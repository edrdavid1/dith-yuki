# Phase 3 Kickoff — Filter Algorithms & Integration

**Status**: ✅ Phase 2 Complete, Phase 3 Ready to Start  
**Date**: July 27, 2026  
**Target Duration**: ~15 hours implementation

---

## Current State

### Phase 2 Completed ✅
- 102 tests passing
- Document model fully operational
- Tauri API with 5 commands (create, add_layer, remove_layer, set_props, reorder)
- FilterInstance structure ready (but algorithms are stubs)
- Coommit 5619dab pushed to origin/main

### What Phase 3 Will Add
- **4 filter algorithms**: Curves, Levels, Dither, Glitch
- **Integration**: Filters hooked into tile generation
- **Tauri commands**: add_filter, update_filter_params, remove_filter, etc.
- **Tests**: 32+ new unit tests + 3+ integration tests
- **Performance**: <100 μs per tile

---

## Your Next Actions

### Option A: Start Immediately with Task 1
```bash
# Task 1: Create filters module structure
# Duration: 1 hour
# Output: empty modules, code compiles

# 1. Create directory
mkdir -p crates/engine-project/src/filters

# 2. Create files:
#    - mod.rs (exports)
#    - curves.rs (empty struct)
#    - levels.rs (empty struct)
#    - dither.rs (empty struct)
#    - glitch.rs (empty struct)
#    - apply.rs (empty dispatcher)

# 3. Update lib.rs to export filters module
# pub mod filters;

# 4. Test: cargo build -p engine-project
```

### Option B: Read Specs First (5 min)
```
Read these files to understand:
- PHASE_3_SPEC_SUMMARY.md (overview, timelines)
- .kiro/specs/.../requirements.md (detailed needs)
- .kiro/specs/.../design.md (algorithms, architecture)
- .kiro/specs/.../tasks.md (task breakdown)
```

### Option C: Start with Task 2 (Curves)
Most impactful: Curves is the simplest filter. Implement it to get productive.

---

## Spec Files Location

All Phase 3 specifications in:
```
.kiro/specs/dither-engine-phase3-filter-algorithms/
├── requirements.md  (9 KB, 200+ lines)
├── design.md        (7 KB, 180+ lines)
└── tasks.md         (6 KB, 150+ lines)
```

Quick links:
- **Wave 1** (foundation): Tasks 1-2
- **Wave 2** (core): Tasks 3-4
- **Wave 3** (effects): Task 5
- **Wave 4** (integration): Tasks 6-7
- **Wave 5** (polish): Task 8

---

## Success Looks Like

✅ Task 1 done: modules created, `cargo build -p engine-project` passes  
✅ Task 2 done: CurvesFilter working, 6 tests pass  
✅ Task 8 done: All filters working, 120+ tests pass, <100 μs latency  

---

## Key Decisions Already Made

1. **Extend engine-project** (not create new crate) — simpler for Phase 3
2. **Catmull-Rom splines** for curves — smooth and fast
3. **Floyd-Steinberg + Ordered dither** — quality + performance
4. **Deterministic glitch** (seeded PRNG) — reproducible effects
5. **Per-filter modules** — clear organization, easy to extend

---

## Commands to Verify State

```bash
# Verify Phase 2 is complete
cargo test --all                    # Should show ~102 tests pass

# Start Phase 3
cargo build -p engine-project       # Baseline: should already pass
cargo clippy --all -- -D warnings   # 0 warnings (Phase 3 won't add any)

# After each task
cargo test -p engine-project        # Verify tests pass
cargo build -p engine-project --release  # Check release build
```

---

## Communication

When you're ready:
- 👉 **Say**: "задача 1" or "Task 1" to start Task 1
- 👉 **Say**: "продолжай" (continue) to move to next task
- 👉 **Say**: "benchmark" to run performance measurements
- 👉 **Say**: "коммит" to create Phase 3 commit

I'll implement autonomously, showing results after each task.

---

## Dependencies & Imports You'll Need

Phase 3 will import:
```rust
use crate::types::*;  // LayerId, FilterInstanceId, BlendMode, etc.
use engine_tiles::{PixelTile, TileCoord, TileCache};  // Phase 1 types
use serde::{Serialize, Deserialize};  // Already available
```

No new external crates needed (curves, levels, dither all use std lib math).

---

## Quick Glossary

- **CurvesFilter**: Tone adjustment via control points
- **LevelsFilter**: Input/output remapping + gamma
- **DitherFilter**: Color reduction (Floyd-Steinberg or Ordered)
- **GlitchFilter**: Creative effects (RGB shift, block displacement)
- **apply_filter_to_tile()**: Dispatcher that calls the right filter
- **FilterStack**: Array of filters applied in sequence to one layer
- **Per-tile latency**: Time to process one 256×256 tile (~1-100 μs)

---

## What's Not Included in Phase 3

❌ **Blur filters** (deferred to Phase 5+)  
❌ **GPU acceleration** (later optimization)  
❌ **UI sliders** (frontend work, Phase 4+)  
❌ **Filter presets** (UI feature, Phase 4+)  
❌ **Full-row pixel sort** (requires_full_row escape hatch, too complex for Phase 3)  

---

## Green Light 🚀

Phase 3 spec is **complete and ready to implement**. You can start immediately or read the specs first.

**Recommendation**: Read PHASE_3_SPEC_SUMMARY.md (5 min), then jump into Task 1.

---

**Ready?** Say "Task 1" or "начнем" to start! 🎯

