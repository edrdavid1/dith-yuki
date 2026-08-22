import type { AppDispatch } from '../app/store';
import { saveProject, saveProjectAs } from '../app/slices/documentSlice';
import { saveDialog } from './ipc';
import type { UnsavedDocumentRef } from './unsavedGuard';

/**
 * Save one open document by runtime id (Photoshop documentID / VS Code URI).
 * Does not depend on which tab is active.
 */
export async function saveUnsavedDocument(
  dispatch: AppDispatch,
  doc: UnsavedDocumentRef
): Promise<boolean> {
  if (doc.path) {
    const result = await dispatch(saveProject({ docId: doc.id, path: doc.path }));
    return saveProject.fulfilled.match(result);
  }
  const filePath = await saveDialog({
    filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
  });
  if (!filePath) return false;
  const path = filePath.toLowerCase().endsWith('.dyproj') ? filePath : `${filePath}.dyproj`;
  const result = await dispatch(saveProjectAs({ docId: doc.id, path }));
  return saveProjectAs.fulfilled.match(result);
}
