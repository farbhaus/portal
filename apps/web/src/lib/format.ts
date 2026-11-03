// Small presentation helpers shared across pages so byte/colour formatting lives in one place.

// Decimal units (1 GB = 1,000,000,000 bytes), the way Frame.io and Finder count, so a file shows
// the same size here as there.
export function formatBytes(n: number): string {
  if (n < 1000) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let v = n / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

// The day a link closes, as its client page shows it ("Expires on 10 November 2026"), in the
// viewer's own time zone.
export function expiryDate(at: string | Date): string {
  return new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" });
}

// Pick readable foreground text (near-black or white) for a hex accent background, by luminance.
export function accentText(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.5 ? "#1a1206" : "#ffffff";
}
