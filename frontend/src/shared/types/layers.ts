/** Layer tree node mirrored from the Rust engine. */
export interface LayerNodeDto {
  id: number;
  name: string;
  kind: 'raster' | 'adjustment' | 'group';
  blend_mode: string;
  opacity: number;
  visible: boolean;
  children?: LayerNodeDto[];
}

/** Partial props patch for `set_layer_props`. */
export interface LayerPropsPatch {
  name?: string;
  opacity?: number;
  blend_mode?: string;
  visible?: boolean;
}
