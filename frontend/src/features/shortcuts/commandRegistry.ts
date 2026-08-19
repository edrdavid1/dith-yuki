export type PreviewCommands = {
  zoomIn: () => void;
  zoomOut: () => void;
  fitToView: () => void;
  actualPixels: () => void;
};

export type DocumentCommands = {
  newProject: () => void;
  openImage: () => void;
  openProject: () => void;
  saveProject: () => void;
  saveProjectAs: () => void;
  openPreferences: () => void;
};

export type LayoutCommands = {
  toggleFocusMode: () => void;
};

let previewCommands: PreviewCommands | null = null;
let documentCommands: DocumentCommands | null = null;
let layoutCommands: LayoutCommands | null = null;

export function registerPreviewCommands(commands: PreviewCommands): () => void {
  previewCommands = commands;
  return () => {
    if (previewCommands === commands) previewCommands = null;
  };
}

export function getPreviewCommands(): PreviewCommands | null {
  return previewCommands;
}

export function registerDocumentCommands(commands: DocumentCommands): () => void {
  documentCommands = commands;
  return () => {
    if (documentCommands === commands) documentCommands = null;
  };
}

export function getDocumentCommands(): DocumentCommands | null {
  return documentCommands;
}

export function registerLayoutCommands(commands: LayoutCommands): () => void {
  layoutCommands = commands;
  return () => {
    if (layoutCommands === commands) layoutCommands = null;
  };
}

export function getLayoutCommands(): LayoutCommands | null {
  return layoutCommands;
}
