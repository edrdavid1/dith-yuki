import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

/** How Pattern Manager should present itself when opened. */
export type PatternManagerIntent = 'browse' | 'save';

export type PatternsUiApi = {
  isOpen: boolean;
  intent: PatternManagerIntent;
  /** Bumps on every open so re-opening with the same intent still applies. */
  intentNonce: number;
  openPatternManager: (intent?: PatternManagerIntent) => void;
  closePatternManager: () => void;
};

const PatternsUiContext = createContext<PatternsUiApi | null>(null);

export function PatternsUiProvider({ children }: { children: ReactNode }) {
  const [isOpen, setIsOpen] = useState(false);
  const [intent, setIntent] = useState<PatternManagerIntent>('browse');
  const [intentNonce, setIntentNonce] = useState(0);

  const openPatternManager = useCallback((next: PatternManagerIntent = 'browse') => {
    setIntent(next);
    setIntentNonce((n) => n + 1);
    setIsOpen(true);
  }, []);

  const closePatternManager = useCallback(() => {
    setIsOpen(false);
  }, []);

  const value = useMemo(
    () => ({
      isOpen,
      intent,
      intentNonce,
      openPatternManager,
      closePatternManager,
    }),
    [closePatternManager, intent, intentNonce, isOpen, openPatternManager]
  );

  return (
    <PatternsUiContext.Provider value={value}>{children}</PatternsUiContext.Provider>
  );
}

export function usePatternsUi(): PatternsUiApi {
  const ctx = useContext(PatternsUiContext);
  if (!ctx) {
    throw new Error('usePatternsUi must be used within PatternsUiProvider');
  }
  return ctx;
}
