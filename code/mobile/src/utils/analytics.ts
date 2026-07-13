// Extra dashboard analytics: performance by holding duration and by trade type.

import type { Trade } from '../api/types';

export interface Bucket {
  label: string;
  count: number;
  wins: number;
  pnl: number;
  winRate: number; // 0..1
}

function parseDate(s: string | null): number | null {
  if (!s) return null;
  const d = new Date(s.replace(' ', 'T'));
  return isNaN(d.getTime()) ? null : d.getTime();
}

/** Holding duration in minutes, or null if we can't compute it. */
function durationMin(t: Trade): number | null {
  const o = parseDate(t.open_time);
  const c = parseDate(t.close_time);
  if (o === null || c === null || c < o) return null;
  return (c - o) / 60000;
}

// --- Risk & discipline metrics ----------------------------------------------

export interface RiskMetrics {
  count: number;
  expectancy: number; // average P&L per trade
  maxDrawdown: number; // largest peak-to-trough drop in cumulative P&L (<= 0)
  bestTrade: number;
  worstTrade: number;
  winStreak: number; // longest run of consecutive wins
  lossStreak: number; // longest run of consecutive losses
  avgHoldMin: number | null; // average holding time (minutes)
}

/** Risk/discipline stats computed from trades (chronological). */
export function riskMetrics(trades: Trade[]): RiskMetrics {
  const rows = trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => ({ ts: parseDate(t.close_time ?? t.open_time ?? t.created_at) ?? 0, pnl: t.pnl as number, hold: durationMin(t) }))
    .sort((a, b) => a.ts - b.ts);

  const count = rows.length;
  if (count === 0) {
    return { count: 0, expectancy: 0, maxDrawdown: 0, bestTrade: 0, worstTrade: 0, winStreak: 0, lossStreak: 0, avgHoldMin: null };
  }

  let sum = 0;
  let peak = 0;
  let cum = 0;
  let maxDrawdown = 0;
  let best = -Infinity;
  let worst = Infinity;
  let curWin = 0;
  let curLoss = 0;
  let winStreak = 0;
  let lossStreak = 0;
  let holdSum = 0;
  let holdCount = 0;

  for (const r of rows) {
    sum += r.pnl;
    cum += r.pnl;
    if (cum > peak) peak = cum;
    if (cum - peak < maxDrawdown) maxDrawdown = cum - peak;
    if (r.pnl > best) best = r.pnl;
    if (r.pnl < worst) worst = r.pnl;
    if (r.pnl > 0) {
      curWin += 1;
      curLoss = 0;
    } else if (r.pnl < 0) {
      curLoss += 1;
      curWin = 0;
    }
    if (curWin > winStreak) winStreak = curWin;
    if (curLoss > lossStreak) lossStreak = curLoss;
    if (r.hold !== null) {
      holdSum += r.hold;
      holdCount += 1;
    }
  }

  return {
    count,
    expectancy: sum / count,
    maxDrawdown,
    bestTrade: best,
    worstTrade: worst,
    winStreak,
    lossStreak,
    avgHoldMin: holdCount ? holdSum / holdCount : null,
  };
}

function bucketOf(mins: number): string {
  if (mins < 5) return '< 5 min';
  if (mins < 30) return '5–30 min';
  if (mins < 60 * 24) return 'Intraday';
  return 'Swing (> 1j)';
}

const DURATION_ORDER = ['< 5 min', '5–30 min', 'Intraday', 'Swing (> 1j)'];

/** Performance grouped by holding-time bucket (only trades with open+close). */
export function durationBuckets(trades: Trade[]): Bucket[] {
  const map = new Map<string, { count: number; wins: number; pnl: number }>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const d = durationMin(t);
    if (d === null) continue;
    const key = bucketOf(d);
    const e = map.get(key) ?? { count: 0, wins: 0, pnl: 0 };
    e.count += 1;
    if ((t.pnl as number) > 0) e.wins += 1;
    e.pnl += t.pnl as number;
    map.set(key, e);
  }
  return DURATION_ORDER.filter((k) => map.has(k)).map((k) => {
    const e = map.get(k)!;
    return { label: k, count: e.count, wins: e.wins, pnl: e.pnl, winRate: e.count ? e.wins / e.count : 0 };
  });
}

export interface Mistake {
  label: string;
  broken: number; // times the rule was answered "no"
  reviewed: number; // times the question was answered (yes or no)
  pnlImpact: number; // total P&L of the trades where it was broken
}

const REVIEW_DIMS: Array<{ field: keyof Trade; label: string }> = [
  { field: 'followed_plan', label: 'Plan non suivi' },
  { field: 'respected_sl', label: 'Stop loss non respecté' },
  { field: 'pattern_valid', label: 'Pattern mal identifié' },
  { field: 'thesis_worked', label: 'Thèse invalidée' },
  { field: 'good_exit', label: 'Sortie mal gérée' },
];

/** Most frequently broken rules, from the per-trade review answers. */
export function topMistakes(trades: Trade[]): Mistake[] {
  return REVIEW_DIMS.map(({ field, label }) => {
    let broken = 0;
    let reviewed = 0;
    let pnlImpact = 0;
    for (const t of trades) {
      const v = t[field] as boolean | null | undefined;
      if (v === true || v === false) reviewed += 1;
      if (v === false) {
        broken += 1;
        if (t.pnl !== null && t.pnl !== undefined) pnlImpact += t.pnl as number;
      }
    }
    return { label, broken, reviewed, pnlImpact };
  })
    .filter((m) => m.broken > 0)
    .sort((a, b) => b.broken - a.broken);
}

export interface SetupPerf {
  name: string;
  count: number;
  wins: number;
  winRate: number;
  pnl: number;
}

/** Performance grouped by setup (uses trades.setup_tag). */
export function setupPerformance(trades: Trade[]): SetupPerf[] {
  const map = new Map<string, { count: number; wins: number; pnl: number }>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const key = t.setup_tag && t.setup_tag.trim() !== '' ? t.setup_tag : 'Sans setup';
    const e = map.get(key) ?? { count: 0, wins: 0, pnl: 0 };
    e.count += 1;
    if ((t.pnl as number) > 0) e.wins += 1;
    e.pnl += t.pnl as number;
    map.set(key, e);
  }
  return [...map.entries()]
    .map(([name, e]) => ({ name, count: e.count, wins: e.wins, winRate: e.count ? e.wins / e.count : 0, pnl: e.pnl }))
    .sort((a, b) => b.pnl - a.pnl);
}

// --- Performance by time-of-day slot (30-min buckets) -----------------------

export interface SlotStat {
  startMin: number; // minutes from midnight
  label: string; // "08:30 PM - 09:00 PM"
  count: number;
  wins: number;
  pnl: number;
  winRate: number; // 0..1
  avgPnl: number;
}

function ampm(min: number): string {
  const h24 = Math.floor(min / 60) % 24;
  const mm = min % 60;
  const suffix = h24 >= 12 ? 'PM' : 'AM';
  let h12 = h24 % 12;
  if (h12 === 0) h12 = 12;
  return `${String(h12).padStart(2, '0')}:${String(mm).padStart(2, '0')} ${suffix}`;
}

function slotLabel(startMin: number): string {
  return `${ampm(startMin)} - ${ampm(startMin + 30)}`;
}

/**
 * Performance grouped into 30-minute slots of the day.
 * `day` filters by day-of-week (0=Sun..6=Sat); null = all days.
 */
export function timeSlotPerformance(
  trades: Trade[],
  day: number | null,
): { slots: SlotStat[]; avgPnl: number; winRate: number; count: number } {
  const map = new Map<number, { count: number; wins: number; pnl: number }>();
  let count = 0;
  let wins = 0;
  let pnl = 0;
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const ms = parseDate(t.open_time ?? t.close_time ?? t.created_at);
    if (ms === null) continue;
    const d = new Date(ms);
    if (day !== null && d.getDay() !== day) continue;
    const slot = d.getHours() * 60 + (d.getMinutes() < 30 ? 0 : 30);
    const e = map.get(slot) ?? { count: 0, wins: 0, pnl: 0 };
    e.count += 1;
    if ((t.pnl as number) > 0) e.wins += 1;
    e.pnl += t.pnl as number;
    map.set(slot, e);
    count += 1;
    if ((t.pnl as number) > 0) wins += 1;
    pnl += t.pnl as number;
  }
  const slots = [...map.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([startMin, e]) => ({
      startMin,
      label: slotLabel(startMin),
      count: e.count,
      wins: e.wins,
      pnl: e.pnl,
      winRate: e.count ? e.wins / e.count : 0,
      avgPnl: e.count ? e.pnl / e.count : 0,
    }));
  return { slots, avgPnl: count ? pnl / count : 0, winRate: count ? wins / count : 0, count };
}

// --- Top symbols traded -----------------------------------------------------

export interface SymPerf {
  symbol: string;
  count: number;
  pnl: number;
}

/** Cumulative P&L per symbol, best first. */
export function symbolPerformance(trades: Trade[]): SymPerf[] {
  const map = new Map<string, { count: number; pnl: number }>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const e = map.get(t.symbol) ?? { count: 0, pnl: 0 };
    e.count += 1;
    e.pnl += t.pnl as number;
    map.set(t.symbol, e);
  }
  return [...map.entries()]
    .map(([symbol, e]) => ({ symbol, count: e.count, pnl: e.pnl }))
    .sort((a, b) => b.pnl - a.pnl);
}

// --- Rolling "trading health" ----------------------------------------------

/**
 * Rolling health score in 0..3 (Reassess=0 .. OutPerforming=3) over the trade
 * sequence, using a trailing window of win-rate + profit-factor.
 */
export function healthSeries(trades: Trade[], window = 8): number[] {
  const seq = trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => ({ ts: parseDate(t.close_time ?? t.open_time ?? t.created_at) ?? 0, pnl: t.pnl as number }))
    .sort((a, b) => a.ts - b.ts);
  if (seq.length < 2) return [];
  const out: number[] = [];
  for (let i = 0; i < seq.length; i++) {
    const win = seq.slice(Math.max(0, i - window + 1), i + 1);
    const n = win.length;
    const w = n ? win.filter((x) => x.pnl > 0).length / n : 0;
    const gp = win.filter((x) => x.pnl > 0).reduce((s, x) => s + x.pnl, 0);
    const gl = Math.abs(win.filter((x) => x.pnl < 0).reduce((s, x) => s + x.pnl, 0));
    const pf = gl > 0 ? gp / gl : gp > 0 ? 3 : 1;
    const score01 = Math.max(0, Math.min(1, 0.55 * w + 0.45 * Math.min(pf / 2, 1)));
    out.push(score01 * 3);
  }
  return out;
}

// --- Per-setup detailed stats -----------------------------------------------

export interface SetupStats {
  count: number;
  wins: number;
  losses: number;
  winRate: number; // 0..1
  lossRate: number; // 0..1
  totalPnl: number;
  avgWin: number;
  avgLoss: number; // negative
  profitFactor: number | null;
  reliability: number; // 0..1, Laplace-smoothed win rate (penalises small samples)
  lastUsed: string | null; // ISO date of most recent trade
}

/** Trades of a given setup (matched by name against setup_tag). */
export function tradesForSetup(trades: Trade[], name: string): Trade[] {
  return trades.filter((t) => (t.setup_tag ?? '').trim() === name.trim());
}

/** Detailed performance stats for one setup. */
export function setupStats(trades: Trade[], name: string): SetupStats {
  const rows = tradesForSetup(trades, name).filter((t) => t.pnl !== null && t.pnl !== undefined);
  let wins = 0;
  let losses = 0;
  let totalPnl = 0;
  let grossWin = 0;
  let grossLoss = 0; // positive magnitude
  let lastTs = -Infinity;
  let lastUsed: string | null = null;
  for (const t of rows) {
    const p = t.pnl as number;
    totalPnl += p;
    if (p > 0) {
      wins += 1;
      grossWin += p;
    } else if (p < 0) {
      losses += 1;
      grossLoss += -p;
    }
    const ref = t.close_time ?? t.open_time ?? t.created_at;
    const ms = parseDate(ref);
    if (ms !== null && ms > lastTs) {
      lastTs = ms;
      lastUsed = ref;
    }
  }
  const count = rows.length;
  const winRate = count ? wins / count : 0;
  return {
    count,
    wins,
    losses,
    winRate,
    lossRate: count ? losses / count : 0,
    totalPnl,
    avgWin: wins ? grossWin / wins : 0,
    avgLoss: losses ? -grossLoss / losses : 0,
    profitFactor: grossLoss > 0 ? grossWin / grossLoss : grossWin > 0 ? null : 0,
    reliability: (wins + 1) / (count + 2), // Laplace smoothing
    lastUsed,
  };
}

export interface EmotionPerf {
  name: string;
  count: number;
  wins: number;
  winRate: number;
  pnl: number;
}

/** Performance grouped by emotion tag (only trades that have one set). */
export function emotionBreakdown(trades: Trade[]): EmotionPerf[] {
  const map = new Map<string, { count: number; wins: number; pnl: number }>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const tag = (t.emotion_tag ?? '').trim();
    if (tag === '') continue;
    const e = map.get(tag) ?? { count: 0, wins: 0, pnl: 0 };
    e.count += 1;
    if ((t.pnl as number) > 0) e.wins += 1;
    e.pnl += t.pnl as number;
    map.set(tag, e);
  }
  return [...map.entries()]
    .map(([name, e]) => ({ name, count: e.count, wins: e.wins, winRate: e.count ? e.wins / e.count : 0, pnl: e.pnl }))
    .sort((a, b) => b.pnl - a.pnl);
}

/** Performance grouped by trade type/direction (LONG/SHORT/CALL/PUT). */
export function typeBreakdown(trades: Trade[]): Bucket[] {
  const map = new Map<string, { count: number; wins: number; pnl: number }>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const key = (t.direction ?? 'N/A').toUpperCase();
    const e = map.get(key) ?? { count: 0, wins: 0, pnl: 0 };
    e.count += 1;
    if ((t.pnl as number) > 0) e.wins += 1;
    e.pnl += t.pnl as number;
    map.set(key, e);
  }
  return [...map.entries()]
    .sort((a, b) => b[1].count - a[1].count)
    .map(([label, e]) => ({ label, count: e.count, wins: e.wins, pnl: e.pnl, winRate: e.count ? e.wins / e.count : 0 }));
}
