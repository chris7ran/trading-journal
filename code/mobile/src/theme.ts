// Dark "web 3.0" theme inspired by TradeZella: deep violet-black surfaces,
// a violet→blue brand gradient, and green/red for P&L.

export const colors = {
  bg: '#0B0B12',
  // Opaque surface for elements that must stay crisp (headers, modal sheets).
  surface: '#15151F',
  // Glass surfaces: translucent whites over the dark bg give a frosted look,
  // with a luminous (light) border. Used for cards, tables, inputs.
  card: 'rgba(255,255,255,0.05)',
  card2: 'rgba(255,255,255,0.09)',
  border: 'rgba(255,255,255,0.12)',
  text: '#ECECF2',
  textMuted: '#8A8A9A',
  primary: '#7C5CFC',
  primary2: '#9D7BFF',
  accentBlue: '#5B8DEF',
  green: '#16C784',
  red: '#F6465D',
  amber: '#F0B90B',
  greenBright: '#22E39A',
  redBright: '#FF5C7A',
  onGradient: '#FFFFFF',
};

/** Reusable gradient color stops (for expo-linear-gradient / SVG). */
export const gradients = {
  brand: ['#7C5CFC', '#5B8DEF'] as const,
};

export const spacing = {
  xs: 4,
  sm: 8,
  md: 16,
  lg: 24,
};

export const radius = {
  md: 10,
  lg: 14,
  xl: 20,
};

/** Format a number as a signed value. */
export function formatPnl(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(2)}`;
}

/** Compact signed format for tight spaces (e.g. +1.8k, -540). */
export function formatPnlCompact(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const sign = value > 0 ? '+' : value < 0 ? '−' : '';
  const abs = Math.abs(value);
  const str = abs >= 1000 ? `${(abs / 1000).toFixed(abs >= 10000 ? 0 : 1)}k` : abs.toFixed(0);
  return `${sign}${str}`;
}

export function pnlColor(value: number | null | undefined): string {
  if (value === null || value === undefined || value === 0) return colors.textMuted;
  return value > 0 ? colors.green : colors.red;
}
