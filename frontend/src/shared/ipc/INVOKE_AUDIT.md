/**
 * Audit of raw `invoke` outside `shared/ipc` (SPEC §9.8 / review close-out).
 *
 * | File | Commands | Args from archive? | Verdict |
 * |---|---|---|---|
 * | `contexts/layoutStore.ts` | `load_shell_layout`, `save_shell_layout`, `layout_flushed` | No — hardcoded names; JSON from FlexLayout models | Safe; W7 SoT |
 * | `spikes/PopoutTestWindow.tsx` | commented `is_release_build` | N/A | Safe; baseline |
 *
 * Debt: optional wrappers in `shared/ipc/flexlayout.ts`.
 * `noRawInvoke.test.ts` allowlists exactly these paths so **new**
 * raw invokes fail CI. Legacy `save_layout_*` / `load_layout_*` are banned
 * from product src by `noLegacyLayoutSave.test.ts`.
 */
export {};
