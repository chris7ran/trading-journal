// Neon "web 3.0" palette + helpers, scoped to the Dashboard redesign.
// Kept separate from the global `theme.ts` so the rest of the app is untouched.

import { Platform } from 'react-native';
import type { ViewStyle } from 'react-native';

export const neon = {
  bg: '#070A10',
  bg2: '#0D1420',
  panel: 'rgba(255,255,255,0.04)',
  panelStrong: 'rgba(255,255,255,0.07)',
  border: 'rgba(255,255,255,0.09)',
  text: '#EAF1F8',
  muted: '#7C8AA0',
  green: '#00FFA3',
  cyan: '#22D3EE',
  violet: '#A855F7',
  pink: '#FF2E93',
  red: '#FF5470',
  track: 'rgba(255,255,255,0.08)',
};

/** iOS glow via shadow (no-op elsewhere). Target platform is iOS. */
export function glow(color: string, radius = 14, opacity = 0.55): ViewStyle {
  if (Platform.OS === 'ios') {
    return { shadowColor: color, shadowOpacity: opacity, shadowRadius: radius, shadowOffset: { width: 0, height: 0 } };
  }
  return {};
}

/** Full currency, e.g. "$102,129". */
export function money(v: number): string {
  return `$${Math.round(v).toLocaleString('en-US')}`;
}

/** Signed currency, e.g. "+$2,129" / "−$540". */
export function moneySigned(v: number): string {
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  return `${sign}$${Math.round(Math.abs(v)).toLocaleString('en-US')}`;
}

/** Compact axis label, e.g. "$102.1k" / "$980". */
export function moneyK(v: number): string {
  const a = Math.abs(v);
  if (a >= 1000) return `$${(v / 1000).toFixed(1)}k`;
  return `$${Math.round(v)}`;
}
