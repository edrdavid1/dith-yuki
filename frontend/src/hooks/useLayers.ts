import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { LayerNodeDto, LayerPropsPatch } from '../components/LayerPanel';

interface UseLayersOptions {
  /** The current document ID; when null, layers are empty. */
  docId: number | null;
}

interface UseLayersReturn {
  layers: LayerNodeDto[];
  selectedLayerId: number | null;
  setSelectedLayerId: (id: number | null) => void;
  addLayer: () => void;
  reorderLayer: (layerId: number, newParent: number | null, newIndex: number) => void;
  setLayerProps: (layerId: number, patch: LayerPropsPatch) => void;
  error: string | null;
}

/**
 * Hook for managing the layer tree state and IPC operations.
 *
 * Fetches the layer tree from the backend on mount and after mutations,
 * provides selection state, and wraps add/reorder/setProps IPC calls.
 */
export function useLayers({ docId }: UseLayersOptions): UseLayersReturn {
  const [layers, setLayers] = useState<LayerNodeDto[]>([]);
  const [selectedLayerId, setSelectedLayerId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Fetch layer tree from backend
  const refreshLayers = useCallback(async () => {
    if (docId === null) {
      setLayers([]);
      return;
    }
    try {
      const tree = await invoke<LayerNodeDto[]>('get_layer_tree');
      setLayers(tree);
      setError(null);
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    }
  }, [docId]);

  // Fetch layers on mount and when docId changes
  useEffect(() => {
    refreshLayers();
  }, [refreshLayers]);

  // Add a new layer above the selected layer (or at top of root)
  const addLayer = useCallback(async () => {
    if (docId === null) return;
    try {
      // Find selected layer's position to insert above it
      const index = findLayerIndex(layers, selectedLayerId);
      await invoke('add_layer', {
        req: {
          kind: 'raster',
          parent_group: null,
          index: index !== null ? index + 1 : layers.length,
        },
      });
      setError(null);
      await refreshLayers();
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    }
  }, [docId, layers, selectedLayerId, refreshLayers]);

  // Reorder a layer to a new position
  const reorderLayer = useCallback(
    async (layerId: number, newParent: number | null, newIndex: number) => {
      if (docId === null) return;
      try {
        await invoke('reorder_layer', {
          req: {
            layer_id: layerId,
            new_parent: newParent,
            new_index: newIndex,
          },
        });
        setError(null);
        await refreshLayers();
      } catch (err) {
        setError(typeof err === 'string' ? err : String(err));
      }
    },
    [docId, refreshLayers]
  );

  // Set layer properties (name, opacity, blend_mode, visible)
  const setLayerPropsHandler = useCallback(
    async (layerId: number, patch: LayerPropsPatch) => {
      if (docId === null) return;
      try {
        await invoke('set_layer_props', {
          req: {
            layer_id: layerId,
            name: patch.name ?? null,
            opacity: patch.opacity ?? null,
            blend_mode: patch.blend_mode ?? null,
            visible: patch.visible ?? null,
          },
        });
        setError(null);
        await refreshLayers();
      } catch (err) {
        setError(typeof err === 'string' ? err : String(err));
      }
    },
    [docId, refreshLayers]
  );

  return {
    layers,
    selectedLayerId,
    setSelectedLayerId,
    addLayer,
    reorderLayer,
    setLayerProps: setLayerPropsHandler,
    error,
  };
}

/**
 * Find the index of a layer by ID in the root-level list.
 * Returns null if not found at root level.
 */
function findLayerIndex(layers: LayerNodeDto[], layerId: number | null): number | null {
  if (layerId === null) return null;
  const idx = layers.findIndex((l) => l.id === layerId);
  return idx >= 0 ? idx : null;
}
