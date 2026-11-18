import { useEffect, useId, useMemo, useRef, useState } from 'react';
import DropdownMenu from '../../../components/common/DropdownMenu';
import { useWorkspaceOptional } from '../../workspace/WorkspaceContext';
import {
  ALGORITHM_FAMILIES,
  getAlgorithmFamily,
  setGpuDitherFamiliesAvailable,
  variantInFamily,
  type AlgorithmFamilyId,
  type AlgorithmSelectorValue,
  type AlgorithmVariantId,
} from './algorithmFamilies';
import {
  isFavorite,
  isHidden,
  readAlgorithmPrefs,
  type AlgorithmPrefs,
} from './algorithmPrefs';
import styles from './AlgorithmSelector.module.css';

export type { AlgorithmSelectorValue };

export interface AlgorithmSelectorProps {
  value: AlgorithmSelectorValue;
  onChange: (next: AlgorithmSelectorValue) => void;
  disabled?: boolean;
}

/**
 * Two linked Chicago-style dropdowns: algorithm family → variant.
 * Per-family last-variant memory is component-lifetime only (§4.2).
 * Favorites / hide persist in localStorage (`dither.algorithmPrefs`).
 */
export function AlgorithmSelector({
  value,
  onChange,
  disabled = false,
}: AlgorithmSelectorProps) {
  const tradeoffId = useId();
  // Match config default (schema ≥2); WorkspaceProvider overrides when ready.
  const gpuDitherFamilies = useWorkspaceOptional()?.gpuDitherFamilies ?? true;
  // Catalog is a module singleton; keep it aligned before we read `available`.
  setGpuDitherFamiliesAvailable(gpuDitherFamilies);

  const [prefs] = useState<AlgorithmPrefs>(() => readAlgorithmPrefs());
  const [showHidden, setShowHidden] = useState(false);

  const lastVariantByFamily = useRef<Partial<Record<AlgorithmFamilyId, AlgorithmVariantId>>>({
    [value.family]: value.variant,
  });

  // Keep memory aligned when the parent drives `value` (e.g. undo / layer switch).
  lastVariantByFamily.current[value.family] = value.variant;

  const activeFamily = getAlgorithmFamily(value.family) ?? ALGORITHM_FAMILIES[0];

  // §4.5 — if the selected family becomes unavailable, fall back to cpu_ed.
  useEffect(() => {
    const family = getAlgorithmFamily(value.family);
    if (!family || family.available) return;
    const fallback = getAlgorithmFamily('cpu_ed')!;
    const nextVariant = variantInFamily(fallback, lastVariantByFamily.current.cpu_ed);
    onChange({ family: 'cpu_ed', variant: nextVariant });
  }, [value.family, onChange, gpuDitherFamilies]);

  // If the selected variant is blocked (e.g. blocked_fs until GPU path), pick a live one.
  useEffect(() => {
    const family = getAlgorithmFamily(value.family);
    if (!family || !family.available) return;
    const current = family.variants.find((variant) => variant.id === value.variant);
    if (current && current.available !== false) return;
    const nextVariant = variantInFamily(family, undefined);
    if (nextVariant === value.variant) return;
    lastVariantByFamily.current[family.id] = nextVariant;
    onChange({ family: family.id, variant: nextVariant });
  }, [value.family, value.variant, onChange, gpuDitherFamilies]);

  // If the current variant was hidden, keep showing it until the user picks another.
  useEffect(() => {
    if (!isHidden(prefs, value.variant)) return;
    if (showHidden) return;
    // Auto-reveal when selection is hidden so the closed field stays truthful.
    setShowHidden(true);
  }, [prefs, value.variant, showHidden]);

  const familyOptions = ALGORITHM_FAMILIES.map((family) => {
    const unavailableReason =
      !family.available && family.id === 'gpu_ed'
        ? 'Disabled: experimental.gpu_dither_families is off in config.json'
        : family.tooltip;
    return {
      value: family.id,
      label: family.available ? family.label : `${family.label} (unavailable)`,
      disabled: !family.available,
      title: unavailableReason,
    };
  });

  const variantOptions = useMemo(() => {
    const family = activeFamily.available
      ? activeFamily
      : (getAlgorithmFamily('cpu_ed') ?? activeFamily);

    const visible = family.variants.filter((variant) => {
      if (variant.available === false) return true; // still listed as disabled
      if (!isHidden(prefs, variant.id)) return true;
      if (showHidden) return true;
      return variant.id === value.variant;
    });

    const favoriteIds = new Set(prefs.favorites);
    const favorites = visible.filter((v) => favoriteIds.has(v.id));
    const rest = visible.filter((v) => !favoriteIds.has(v.id));
    const ordered = [...favorites, ...rest];

    return ordered.map((variant) => {
      const available = variant.available !== false;
      const unavailableReason =
        !available &&
        (variant.id === 'blocked_fs' ||
          variant.id === 'blue_noise' ||
          variant.id === 'dot_diffusion')
          ? 'Disabled: experimental.gpu_dither_families is off in config.json'
          : (variant.tooltip ?? '');
      const fav = isFavorite(prefs, variant.id);
      const hid = isHidden(prefs, variant.id);
      let label = available ? variant.label : `${variant.label} (unavailable)`;
      if (fav) label = `★ ${label}`;
      if (hid) label = `${label} (hidden)`;
      return {
        value: variant.id,
        label,
        title: unavailableReason || variant.tooltip || variant.label,
        disabled: !available,
        group: fav ? 'Favorites' : undefined,
      };
    });
  }, [activeFamily, prefs, showHidden, value.variant]);

  const handleFamilySelect = (familyId: string) => {
    const family = getAlgorithmFamily(familyId as AlgorithmFamilyId);
    if (!family || !family.available) return;
    lastVariantByFamily.current[value.family] = value.variant;
    const nextVariant = variantInFamily(family, lastVariantByFamily.current[family.id]);
    lastVariantByFamily.current[family.id] = nextVariant;
    onChange({ family: family.id, variant: nextVariant });
  };

  const handleVariantSelect = (variantId: string) => {
    const nextVariant = variantId as AlgorithmVariantId;
    const entry = activeFamily.variants.find((variant) => variant.id === nextVariant);
    if (!entry || entry.available === false) return;
    lastVariantByFamily.current[activeFamily.id] = nextVariant;
    onChange({ family: activeFamily.id, variant: nextVariant });
  };

  const displayFamily = activeFamily.available ? value.family : 'cpu_ed';
  const displayTradeoff = activeFamily.available
    ? activeFamily.tradeoff
    : (getAlgorithmFamily('cpu_ed')?.tradeoff ?? activeFamily.tradeoff);

  return (
    <div className={styles.root}>
      <DropdownMenu
        label="Algorithm family"
        value={displayFamily}
        options={familyOptions}
        onSelect={handleFamilySelect}
        disabled={disabled}
        ariaLabel="Dither algorithm family"
        ariaDescribedBy={tradeoffId}
      />
      <p
        id={tradeoffId}
        className={`${styles.tradeoff}${disabled ? ` ${styles.tradeoffDisabled}` : ''}`}
      >
        {displayTradeoff}
      </p>
      <DropdownMenu
        label="Algorithm"
        value={activeFamily.available ? value.variant : 'floyd_steinberg'}
        options={variantOptions}
        onSelect={handleVariantSelect}
        disabled={disabled}
        ariaLabel="Dither algorithm variant"
      />
      {/* Favorite / Hide / Show-hidden controls kept in algorithmPrefs; UI closed for now. */}
    </div>
  );
}

export default AlgorithmSelector;
