import { invoke } from '@tauri-apps/api/core';

export interface SetViewportArgs {
  zoom: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export async function setViewport(args: SetViewportArgs): Promise<void> {
  return invoke<void>('set_viewport', {
    zoom: args.zoom,
    x: args.x,
    y: args.y,
    width: args.width,
    height: args.height,
  });
}
