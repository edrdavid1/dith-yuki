import { useContext } from 'react';
import { ShellContext } from '../../app/shell/ShellContext';

/**
 * Hover tips and inline interface hints.
 * On when no shell is mounted so unit tests keep the explanatory copy.
 */
export function useTooltipsEnabled(): boolean {
  return useContext(ShellContext)?.tooltipsEnabled ?? true;
}
