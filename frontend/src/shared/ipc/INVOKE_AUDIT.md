/**
 * Audit of raw `invoke` outside `shared/ipc` (SPEC §9.8 / review close-out).
 *
 * | File | Commands | Args from archive? | Verdict |
 * |---|---|---|---|
 * | `contexts/LayoutContext.tsx` | `load_layout_left/right`, `save_layout_left/right` | No — hardcoded names; JSON from FlexLayout model | Safe; baseline |
 * | `components/FlexLayoutContainer.tsx` | `save_layout_left/right` | No — same | Safe; baseline |
 * | `spikes/PopoutTestWindow.tsx` | commented `is_release_build` | N/A | Safe; baseline |
 *
 * Debt: migrate these to `shared/ipc/flexlayout.ts` (or similar) later.
 * Until then `noRawInvoke.test.ts` allowlists exactly these paths so **new**
 * raw invokes fail CI.
 */
export {};
