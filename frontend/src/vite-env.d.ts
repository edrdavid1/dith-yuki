/// <reference types="vite/client" />

declare module 'virtual:flexlayout-patch' {
  export type FlexPatchMarker = {
    version: number;
    features: string[];
  };
  export const marker: FlexPatchMarker | null;
  export const ok: boolean;
}
