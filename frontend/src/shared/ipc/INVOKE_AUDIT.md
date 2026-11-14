/**
 * Audit of raw `invoke` outside `shared/ipc` (SPEC §9.8 / review close-out).
 *
 * | File | Commands | Args from archive? | Verdict |
 * |---|---|---|---|
 * | `contexts/layoutStore.ts` | `load_shell_layout`, `save_shell_layout`, `layout_flushed` | No — hardcoded names; JSON from FlexLayout models | Safe; W7 SoT |
 *
 * Channel and boot commands go through `shared/ipc/channel.ts` and
 * `shared/ipc/boot.ts`. Diagnostics remain an explicit exception in
 * `noRawInvoke.test.ts`.
 *
 * Debt: optional wrappers in `shared/ipc/flexlayout.ts`.
 * `noRawInvoke.test.ts` allowlists exactly the path above so **new**
 * raw invokes fail CI. Legacy `save_layout_*` / `load_layout_*` are banned
 * from product src by `noLegacyLayoutSave.test.ts`.
 */
export {};
