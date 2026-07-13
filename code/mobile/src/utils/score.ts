// A composite "discipline" score (0–100), inspired by TradeZella's Zella Score.
//
// It blends three signals derived from the aggregated stats:
//   - win rate (how often you win)
//   - profit factor (how much you win vs lose), capped at 3 => 100%
//   - profitability (net positive bonus)
// This is a heuristic for at-a-glance feedback, not a financial metric.

import type { TradeStats } from '../api/types';

export function computeScore(stats: TradeStats | null): number {
  if (!stats || stats.total_trades === 0) return 0;

  const winRate = clamp01(stats.win_rate);
  const pf = stats.profit_factor === null ? (stats.gross_loss === 0 ? 3 : 0) : stats.profit_factor;
  const pfScore = clamp01(pf / 3);
  const positive = stats.total_pnl > 0 ? 1 : 0;

  const raw = 0.45 * winRate + 0.45 * pfScore + 0.1 * positive;
  return Math.round(clamp01(raw) * 100);
}

/** A short qualitative label for the score. */
export function scoreLabel(score: number): string {
  if (score >= 75) return 'Excellent';
  if (score >= 60) return 'Solide';
  if (score >= 45) return 'À affiner';
  return 'Fragile';
}

function clamp01(x: number): number {
  if (Number.isNaN(x)) return 0;
  return Math.max(0, Math.min(1, x));
}
