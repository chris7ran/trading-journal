// Prop firm rule tracking: profit target, global drawdown, daily drawdown.
// All thresholds are fractions of the starting balance (e.g. 0.05 = 5%).

import type { PropRule, Trade } from '../api/types';

export type AlertLevel = 'ok' | 'warn' | 'danger';

export interface PropAlert {
  level: AlertLevel;
  text: string;
}

export interface PropStatus {
  startBalance: number;
  netPnl: number;
  currentEquity: number;
  peakEquity: number;

  profitTarget: number | null; // $ amount
  profitProgress: number; // 0..1+
  targetReached: boolean;

  maxDrawdown: number | null; // $ allowed from peak
  currentDrawdown: number; // $ below peak (>= 0)
  drawdownUsed: number; // 0..1+

  dailyLimit: number | null; // $ allowed loss today
  todayPnl: number;
  dailyUsed: number; // 0..1+

  // Consistency rule: best single day must stay below `consistencyLimit` of total.
  consistencyLimit: number; // fraction, e.g. 0.20
  bestDayPnl: number;
  consistencyPct: number; // bestDay / net (0..1+), 0 if net <= 0
  consistencyOk: boolean;
  minTotalForPayout: number; // bestDay / limit

  alerts: PropAlert[];
}

function tradeDay(t: Trade): string {
  const raw = (t.close_time ?? t.open_time ?? t.created_at).replace(' ', 'T');
  return raw.slice(0, 10);
}

export function computePropStatus(
  trades: Trade[],
  startBalance: number,
  rule: PropRule | null,
): PropStatus {
  const closed = trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => ({ day: tradeDay(t), pnl: t.pnl as number }))
    .sort((a, b) => a.day.localeCompare(b.day));

  let equity = startBalance;
  let peak = startBalance;
  let net = 0;
  for (const c of closed) {
    equity += c.pnl;
    net += c.pnl;
    if (equity > peak) peak = equity;
  }
  const currentEquity = startBalance + net;
  const currentDrawdown = Math.max(0, peak - currentEquity);

  const today = new Date().toISOString().slice(0, 10);
  const todayPnl = closed.filter((c) => c.day === today).reduce((s, c) => s + c.pnl, 0);

  // Consistency: best single trading day vs total profit.
  const byDay = new Map<string, number>();
  for (const c of closed) byDay.set(c.day, (byDay.get(c.day) ?? 0) + c.pnl);
  let bestDayPnl = 0;
  for (const v of byDay.values()) if (v > bestDayPnl) bestDayPnl = v;
  const consistencyLimit = rule?.consistency_rule_pct ?? 0.2;
  const consistencyPct = net > 0 ? bestDayPnl / net : 0;
  const consistencyOk = net <= 0 ? true : bestDayPnl <= consistencyLimit * net;
  const minTotalForPayout = consistencyLimit > 0 ? bestDayPnl / consistencyLimit : 0;

  const profitTarget = rule?.profit_target != null ? startBalance * rule.profit_target : null;
  const maxDrawdown = rule?.global_drawdown_max != null ? startBalance * rule.global_drawdown_max : null;
  const dailyLimit = rule?.daily_drawdown_max != null ? startBalance * rule.daily_drawdown_max : null;

  const profitProgress = profitTarget && profitTarget > 0 ? net / profitTarget : 0;
  const drawdownUsed = maxDrawdown && maxDrawdown > 0 ? currentDrawdown / maxDrawdown : 0;
  const dailyLoss = todayPnl < 0 ? -todayPnl : 0;
  const dailyUsed = dailyLimit && dailyLimit > 0 ? dailyLoss / dailyLimit : 0;

  const alerts: PropAlert[] = [];
  if (maxDrawdown != null) {
    if (drawdownUsed >= 1) alerts.push({ level: 'danger', text: 'Drawdown global DÉPASSÉ.' });
    else if (drawdownUsed >= 0.8) alerts.push({ level: 'warn', text: 'Drawdown global proche de la limite.' });
  }
  if (dailyLimit != null) {
    if (dailyUsed >= 1) alerts.push({ level: 'danger', text: 'Limite de perte journalière DÉPASSÉE.' });
    else if (dailyUsed >= 0.8) alerts.push({ level: 'warn', text: 'Perte journalière proche de la limite.' });
  }
  if (profitTarget != null && net >= profitTarget) {
    alerts.push({ level: 'ok', text: 'Objectif de profit atteint 🎯' });
  }
  if (net > 0 && !consistencyOk) {
    alerts.push({
      level: 'warn',
      text: `Consistance ${Math.round(consistencyPct * 100)}% > ${Math.round(consistencyLimit * 100)}% — payout bloqué (total requis ≥ ${Math.round(minTotalForPayout)}).`,
    });
  }
  if (alerts.length === 0) alerts.push({ level: 'ok', text: 'Tout est dans les règles.' });

  return {
    startBalance,
    netPnl: net,
    currentEquity,
    peakEquity: peak,
    profitTarget,
    profitProgress,
    targetReached: profitTarget != null && net >= profitTarget,
    maxDrawdown,
    currentDrawdown,
    drawdownUsed,
    dailyLimit,
    todayPnl,
    dailyUsed,
    consistencyLimit,
    bestDayPnl,
    consistencyPct,
    consistencyOk,
    minTotalForPayout,
    alerts,
  };
}
