import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

export type PresetManagerIntent = 'browse' | 'save';

export type PresetsUiApi = {
  isOpen: boolean;
  intent: PresetManagerIntent;
  intentNonce: number;
  openPresetManager: (intent?: PresetManagerIntent) => void;
  closePresetManager: () => void;
};

const PresetsUiContext = createContext<PresetsUiApi | null>(null);

export function PresetsUiProvider({ children }: { children: ReactNode }) {
  const [isOpen, setIsOpen] = useState(false);
  const [intent, setIntent] = useState<PresetManagerIntent>('browse');
  const [intentNonce, setIntentNonce] = useState(0);

  const openPresetManager = useCallback((next: PresetManagerIntent = 'browse') => {
    setIntent(next);
    setIntentNonce((n) => n + 1);
    setIsOpen(true);
  }, []);

  const closePresetManager = useCallback(() => {
    setIsOpen(false);
  }, []);

  const value = useMemo(
    () => ({
      isOpen,
      intent,
      intentNonce,
      openPresetManager,
      closePresetManager,
    }),
    [closePresetManager, intent, intentNonce, isOpen, openPresetManager]
  );

  return (
    <PresetsUiContext.Provider value={value}>{children}</PresetsUiContext.Provider>
  );
}

export function usePresetsUi(): PresetsUiApi {
  const ctx = useContext(PresetsUiContext);
  if (!ctx) {
    throw new Error('usePresetsUi must be used within PresetsUiProvider');
  }
  return ctx;
}
