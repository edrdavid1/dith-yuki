/**
 * @deprecated Import from `../shared/ipc` (or specific modules) instead.
 * Kept as a compatibility barrel for existing imports/tests.
 */
export {
  loadImage,
  exportImage,
  getDocumentSnapshot,
} from '../shared/ipc/document';
export type {
  DocumentSnapshotResponse,
  SnapshotLayerNode,
  SnapshotFilterInfo,
} from '../shared/ipc/document';

export {
  getLayerTree,
  addLayer,
  removeLayer,
  reorderLayer,
  setLayerProps,
} from '../shared/ipc/layers';

export {
  addFilter,
  updateFilter,
  removeFilter,
  reorderFilter,
} from '../shared/ipc/filters';

export type { PaletteDto, DeletePaletteResponse, BuiltinPaletteDto } from '../shared/ipc/palettes';
export {
  listPalettes,
  listBuiltinPalettes,
  importBuiltinPalette,
  importPalette,
  addPalette,
  generatePalette,
  removePalette,
  createPalette,
  deletePalette,
  addColorToPalette,
  updatePaletteColor,
  removePaletteColor,
  reorderPaletteColor,
  renamePalette,
  exportPalette,
} from '../shared/ipc/palettes';
