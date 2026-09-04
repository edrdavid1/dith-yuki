# FlexLayout Integration Guide (Phase 2.5)

> **Устарело.** Актуальная документация: [`docs/FLEXLAYOUT_DOCKING.md`](../../docs/FLEXLAYOUT_DOCKING.md)  
> (Layers **и** Effect уже на FlexLayout; этот гайд описывает только B3 Layers-only.)

## Overview

This guide explains how to integrate the FlexLayout-based Layers panel into the main app layout during Phase 2.5.

## Current Architecture (AppLayout.tsx)

The main app has:
- **Titlebar** with MenuBar
- **Main grid layout**:
  - Left sidebar (DockedSidebar with effect/colorlab)
  - Center canvas (PreviewSlot)
  - Right sidebar (DockedSidebar with effect/colorlab)

## B3 Integration Strategy

During B3:
- **Layers panel migrates to FlexLayout** (separate from DockedSidebar)
- **Effect & colorlab remain on old PanelManager** (in DockedSidebar)
- Both systems run in parallel (coexistence)

### Option A: Replace Left Sidebar (Recommended)

The left sidebar currently renders "effect" panels via old PanelManager. Simplest approach:

1. Check if left sidebar would render only "layers" (or empty)
2. If so: render `<LayoutProvider><FlexLayoutContainer /></LayoutProvider>` instead of `<DockedSidebar>`
3. Keep right sidebar as-is (effect/colorlab panels)

**Pros:**
- Minimal changes to AppLayout
- Clear visual separation: new (left) vs old (right)
- No need to restructure existing sidebar code

**Cons:**
- Assumes left sidebar holds layers (may not always be true if user swaps)

### Option B: Side-by-Side (Future-Proof)

Render both systems side-by-side in a tabbed interface:

1. Add tabs above the main grid: "Layout" (FlexLayout) vs "Panels" (old DockedSidebar)
2. Show/hide based on selected tab
3. Preserve both systems without conflict

**Pros:**
- Works regardless of user sidebar config
- Explicit UI for "new" vs "old" panels
- Clean transition path for B4 (add effect/colorlab tabs to FlexLayout)

**Cons:**
- More UI changes
- Requires additional state management

### Option C: Overlay (Advanced)

Render FlexLayout in a floating panel over the old layout:

1. Add button to AppLayout: "Open New Layout"
2. FloatingWindow or modal shows FlexLayout
3. User can switch back to old layout anytime

**Pros:**
- Zero changes to existing layout
- User can compare old vs new side-by-side

**Cons:**
- Complex to implement
- May confuse users

## Recommended Implementation (Option A)

### Step 1: Update AppLayout Imports

```typescript
// Add to imports
import { LayoutProvider } from '../contexts/LayoutContext';
import { FlexLayoutContainer } from '../components/FlexLayoutContainer';
```

### Step 2: Conditionally Render

In the main grid, replace the left sidebar:

```typescript
{!focusMode && leftPanels.length > 0 && leftPanels.some(id => id === 'layers') ? (
  // NEW: FlexLayout for Layers panel (B3)
  <LayoutProvider>
    <FlexLayoutContainer panelsToRender={['layers']} />
  </LayoutProvider>
) : !focusMode && (
  // OLD: DockedSidebar for effect/colorlab (B3, B4 legacy)
  <DockedSidebar
    side="left"
    // ... existing props
  />
)}
```

### Step 3: Handle Edge Cases

- **User swaps sidebars**: Layers might move to right. Handle by checking both `leftPanels` and `rightPanels`
- **All panels hidden (focus mode)**: FlexLayout gracefully handles `isLoading` state
- **v2 migration**: LayoutProvider shows default layout automatically

## Installation Prerequisites

Before Phase 2.5 can be completed, you need to add the FlexLayout library:

```bash
npm install flexlayout-react
# or
yarn add flexlayout-react
```

Update `vite.config.ts` to handle FlexLayout (if needed):

```typescript
// vite.config.ts
export default defineConfig({
  optimizeDeps: {
    include: ['flexlayout-react'],
  },
});
```

## Files Involved

- `AppLayout.tsx` — main app shell (needs modification)
- `LayoutContext.tsx` — ✅ context provider (created in Phase 2.2)
- `FlexLayoutContainer.tsx` — ✅ layout wrapper (created in Phase 2.3)
- `layoutPanelFactory.tsx` — ✅ panel factory (created in Phase 2.4)
- `DefaultLayouts.ts` — ✅ layout schemas (created in Phase 2.1)

## Testing Checklist

After integration:

- [ ] LayoutProvider loads without errors
- [ ] FlexLayoutContainer renders with default layout (Layers docked left)
- [ ] Loading spinner shows briefly on app start
- [ ] Layout persists after app restart
- [ ] v2 layout file triggers migration (if present)
- [ ] Corrupt layout falls back to default
- [ ] Layers panel remains functional inside FlexLayout
- [ ] Old DockedSidebar (right) still works for effect/colorlab
- [ ] Swap sidebars correctly routes Layers to FlexLayout
- [ ] No console errors

## Future Phases (B4+)

### B4: Migrate Effect Panel

1. Add `effect` to FlexLayout in `layoutPanelFactory.tsx`
2. Remove `effect` from old DockedSidebar
3. Update default layout to include effect tab

### B4: Migrate ColorLab Panel

Same as effect; repeat process.

### B4: Remove Old PanelManager

Once both effect and colorlab are on FlexLayout:

1. Delete `dock_affinity.rs`, `global_mouseup.rs`
2. Remove 15 old panel commands from Tauri
3. Delete `panel_manager.rs`, `panel_persistence.rs`
4. Remove `DockedSidebar` component

## Performance Considerations

- **First load**: ~200-300ms (JSON parsing + React render)
- **Debounced save**: 500ms delay to avoid excessive disk writes
- **Memory**: FlexLayout library ~50-100KB gzipped (check bundle size)

## Debugging

If FlexLayout doesn't appear:

1. **Check browser console** for errors (DevTools or `npm run dev`)
2. **Verify Tauri commands** are working:
   ```bash
   # In DevTools console:
   await invoke('load_layout')
   await invoke('save_layout', { json: '{"version":3,"root":{}}' })
   ```
3. **Check flexlayout_state.json** exists in `~/.app_data_dir/` after restart
4. **Verify LayoutProvider wraps FlexLayoutContainer** (context must be available)

## Known Limitations (B3)

- No popout/floating windows (depends on B2 spike, Phase 3.1)
- Effect & colorlab still on old system (migrate in B4)
- No live reload in Vite dev mode (restart app to see layout changes)

## References

- FlexLayout documentation: https://caplin.github.io/FlexLayout/
- B2 ADR: `docs/B2_ADR_flexlayout_docking.md`
- B3 Tasks: `.cursor-spec/track-r-docking/B3_tasks.md`
- Audit (as-built): `.cursor-spec/track-b-infra/AUDIT_docking_current_implementation.md`
