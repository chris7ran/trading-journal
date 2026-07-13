// Build chart series from the raw trade list.

import type { Trade } from '../api/types';

/** A closed trade reduced to a sortable date key + its P&L. */
interface Point {
  key: string; // normalized ISO-ish string, lexicographically sortable
  pnl: number;
}

function tradeDate(t: Trade): string | null {
  const raw = t.close_time ?? t.open_time ?? t.created_at;
  return raw ? raw.replace(' ', 'T') : null;
}

function closedPoints(trades: Trade[]): Point[] {
  return trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined && tradeDate(t) !== null)
    .map((t) => ({ key: tradeDate(t) as string, pnl: t.pnl as number }))
    .sort((a, b) => a.key.localeCompare(b.key));
}

/**
 * Cumulative P&L after each closed trade, in chronological order.
 * Returns an empty array if there are no closed trades.
 */
export function equitySeries(trades: Trade[]): number[] {
  const pts = closedPoints(trades);
  let cum = 0;
  return pts.map((p) => (cum += p.pnl));
}

export interface DailyPnl {
  day: string; // YYYY-MM-DD
  value: number;
}

/** Sum of P&L per calendar day, chronological. */
export function dailyPnlSeries(trades: Trade[]): DailyPnl[] {
  const byDay = new Map<string, number>();
  for (const p of closedPoints(trades)) {
    const day = p.key.slice(0, 10);
    byDay.set(day, (byDay.get(day) ?? 0) + p.pnl);
  }
  return Array.from(byDay.entries())
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([day, value]) => ({ day, value }));
}
