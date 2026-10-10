import { useEffect, useId, useRef } from 'react';
import DropdownMenu from '../../../components/common/DropdownMenu';
import {
  ALGORITHM_FAMILIES,
  getAlgorithmFamily,
  variantInFamily,
  type AlgorithmFamilyId,
  type AlgorithmSelectorValue,
  type AlgorithmVariantId,
} from './algorithmFamilies';
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
 */
export function AlgorithmSelector({
  value,
  onChange,
  disabled = false,
}: AlgorithmSelectorProps) {
  const tradeoffId = useId();
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
  }, [value.family, onChange]);

  const familyOptions = ALGORITHM_FAMILIES.map((family) => ({
    value: family.id,
    label: family.available ? family.label : `${family.label} (unavailable)`,
    disabled: !family.available,
    title: family.tooltip,
  }));

  const variantOptions = activeFamily.variants.map((variant) => ({
    value: variant.id,
    label: variant.label,
    title: variant.tooltip ?? '',
  }));

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
    if (!activeFamily.variants.some((variant) => variant.id === nextVariant)) return;
    lastVariantByFamily.current[activeFamily.id] = nextVariant;
    onChange({ family: activeFamily.id, variant: nextVariant });
  };

  const displayFamily = activeFamily.available ? value.family : 'cpu_ed';
  const displayTradeoff = activeFamily.available
    ? activeFamily.tradeoff
    : (getAlgorithmFamily('cpu_ed')?.tradeoff ?? activeFamily.tradeoff);
  const displayVariants = activeFamily.available
    ? variantOptions
    : (getAlgorithmFamily('cpu_ed')?.variants.map((variant) => ({
        value: variant.id,
        label: variant.label,
        title: variant.tooltip ?? '',
      })) ?? variantOptions);

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
        options={displayVariants}
        onSelect={handleVariantSelect}
        disabled={disabled}
        ariaLabel="Dither algorithm variant"
      />
    </div>
  );
}

export default AlgorithmSelector;
