/** Map space-separated global-style names onto a CSS module class map. */
export function bind(styles: Record<string, string>) {
  return (...parts: Array<string | false | null | undefined>) =>
    parts
      .filter(Boolean)
      .flatMap((p) => String(p).split(/\s+/))
      .filter(Boolean)
      .map((name) => styles[name] ?? name)
      .join(' ');
}

export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(' ');
}
