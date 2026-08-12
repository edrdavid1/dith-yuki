/**
 * Hex format conversion utilities for bridging between the frontend `#rrggbb`
 * display format and the backend 6-character uppercase hex format (no prefix).
 */

const HEX_WITH_HASH = /^#[0-9a-fA-F]{6}$/;
const HEX_WITHOUT_HASH = /^[0-9a-fA-F]{6}$/;
const HEX_OPTIONAL_HASH = /^#?[0-9a-fA-F]{6}$/;

/**
 * Convert frontend display hex (#rrggbb or #RRGGBB) to backend format (RRGGBB uppercase, no prefix).
 * Throws if input is not a valid 7-char hex string with "#" prefix.
 */
export function hexToBackend(displayHex: string): string {
  if (typeof displayHex !== "string" || !HEX_WITH_HASH.test(displayHex)) {
    throw new Error(
      `Invalid display hex color: "${displayHex}". Expected format: #RRGGBB (7 characters with "#" prefix).`
    );
  }
  return displayHex.slice(1).toUpperCase();
}

/**
 * Convert backend hex (6-char, with or without "#") to frontend display format (#rrggbb lowercase).
 * Throws if input is not a valid 6-char hex string (optionally prefixed with "#").
 */
export function hexToDisplay(backendHex: string): string {
  if (typeof backendHex !== "string" || !HEX_OPTIONAL_HASH.test(backendHex)) {
    throw new Error(
      `Invalid backend hex color: "${backendHex}". Expected 6 hex characters, optionally prefixed with "#".`
    );
  }
  const raw = backendHex.startsWith("#") ? backendHex.slice(1) : backendHex;
  return `#${raw.toLowerCase()}`;
}
