// Rule-based "coach": derive actionable insights from the trade history.
// No external AI/API — everything is computed locally from the data.

import type { Trade } from '../api/types';

export type InsightLevel = 'good' | 'warn' | 'info';

export interface Insight {
  level: InsightLevel;
  title: string;
  detail: string;
}

interface C {
  pnl: number;
  symbol: string;
  hour: number | null;
  emotion: string | null;
  day: string;
}

function fmt(n: number): string {
  const s = n > 0 ? '+' : '';
  return `${s}${Math.round(n).toLocaleString('fr-FR')}`;
}

function toClosed(trades: Trade[]): C[] {
  return trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => {
      const raw = (t.close_time ?? t.open_time ?? t.created_at).replace(' ', 'T');
      const d = new Date(raw);
      const valid = !isNaN(d.getTime());
      return {
        pnl: t.pnl as number,
        symbol: t.symbol,
        hour: valid ? d.getHours() : null,
        emotion: t.emotion_tag ?? null,
        day: raw.slice(0, 10),
      };
    })
    .sort((a, b) => a.day.localeCompare(b.day));
}

export function computeInsights(trades: Trade[]): Insight[] {
  const cs = toClosed(trades);
  if (cs.length < 3) {
    return [
      {
        level: 'info',
        title: 'Pas encore assez de trades',
        detail: 'Ajoute ou importe quelques trades clôturés pour débloquer les analyses du coach.',
      },
    ];
  }

  const out: Insight[] = [];
  const total = cs.reduce((s, c) => s + c.pnl, 0);
  const wins = cs.filter((c) => c.pnl > 0).length;
  const winRate = wins / cs.length;

  out.push({
    level: total >= 0 ? 'good' : 'warn',
    title: `P&L total ${fmt(total)}`,
    detail: `${cs.length} trades clôturés · win rate ${(winRate * 100).toFixed(0)}%.`,
  });

  // --- Risk:reward ---
  const winsArr = cs.filter((c) => c.pnl > 0).map((c) => c.pnl);
  const lossArr = cs.filter((c) => c.pnl < 0).map((c) => c.pnl);
  if (winsArr.length && lossArr.length) {
    const avgWin = winsArr.reduce((a, b) => a + b, 0) / winsArr.length;
    const avgLoss = Math.abs(lossArr.reduce((a, b) => a + b, 0) / lossArr.length);
    const rr = avgLoss > 0 ? avgWin / avgLoss : 0;
    out.push({
      level: rr >= 1.5 ? 'good' : rr < 1 ? 'warn' : 'info',
      title: `Ratio gain/perte ${rr.toFixed(2)}`,
      detail:
        rr < 1
          ? `Gain moyen ${fmt(avgWin)} < perte moyenne ${fmt(-avgLoss)} : tes pertes effacent tes gains.`
          : `Gain moyen ${fmt(avgWin)} vs perte moyenne ${fmt(-avgLoss)}.`,
    });
  }

  // --- Per-instrument win rate (>= 3 trades) ---
  const bySym = new Map<string, { n: number; w: number; pnl: number }>();
  for (const c of cs) {
    const e = bySym.get(c.symbol) ?? { n: 0, w: 0, pnl: 0 };
    e.n += 1;
    if (c.pnl > 0) e.w += 1;
    e.pnl += c.pnl;
    bySym.set(c.symbol, e);
  }
  const symStats = [...bySym.entries()].filter(([, e]) => e.n >= 3);
  if (symStats.length >= 2) {
    const sorted = [...symStats].sort((a, b) => b[1].w / b[1].n - a[1].w / a[1].n);
    const [bs, be] = sorted[0];
    const [ws, we] = sorted[sorted.length - 1];
    out.push({
      level: 'good',
      title: `Ton meilleur instrument : ${bs}`,
      detail: `${((be.w / be.n) * 100).toFixed(0)}% de réussite sur ${be.n} trades (${fmt(be.pnl)}).`,
    });
    if (we.w / we.n < 0.45) {
      out.push({
        level: 'warn',
        title: `À surveiller : ${ws}`,
        detail: `Seulement ${((we.w / we.n) * 100).toFixed(0)}% de réussite sur ${we.n} trades (${fmt(we.pnl)}).`,
      });
    }
  }

  // --- Revenge trading: outcome of the trade after a loss ---
  let afterLoss = 0;
  let afterLossLoss = 0;
  for (let i = 1; i < cs.length; i++) {
    if (cs[i - 1].pnl < 0) {
      afterLoss += 1;
      if (cs[i].pnl < 0) afterLossLoss += 1;
    }
  }
  if (afterLoss >= 5) {
    const ratio = afterLossLoss / afterLoss;
    if (ratio > 0.55) {
      out.push({
        level: 'warn',
        title: 'Attention au revenge-trading',
        detail: `Après une perte, ton trade suivant est perdant ${(ratio * 100).toFixed(0)}% du temps. Fais une pause après une perte.`,
      });
    } else {
      out.push({
        level: 'good',
        title: 'Bonne gestion après une perte',
        detail: `Après une perte, tu restes gagnant ${(100 - ratio * 100).toFixed(0)}% du temps.`,
      });
    }
  }

  // --- Consistency: best single day vs total ---
  const byDay = new Map<string, number>();
  for (const c of cs) byDay.set(c.day, (byDay.get(c.day) ?? 0) + c.pnl);
  let bestDay = 0;
  for (const v of byDay.values()) if (v > bestDay) bestDay = v;
  if (total > 0 && bestDay > 0) {
    const pct = bestDay / total;
    if (pct > 0.3) {
      out.push({
        level: 'warn',
        title: `Profits concentrés (${(pct * 100).toFixed(0)}% sur un jour)`,
        detail: `Ton meilleur jour (${fmt(bestDay)}) pèse beaucoup dans ton total. Pour une prop firm, vise plus de régularité.`,
      });
    }
  }

  // --- Max losing streak ---
  let streak = 0;
  let maxStreak = 0;
  for (const c of cs) {
    if (c.pnl < 0) {
      streak += 1;
      maxStreak = Math.max(maxStreak, streak);
    } else {
      streak = 0;
    }
  }
  if (maxStreak >= 3) {
    out.push({
      level: 'info',
      title: `Série de pertes max : ${maxStreak}`,
      detail: 'Plan de gestion du risque utile pour encaisser ces séries sans casser ton compte.',
    });
  }

  // --- Time of day ---
  const morning = cs.filter((c) => c.hour !== null && c.hour < 13);
  const afternoon = cs.filter((c) => c.hour !== null && c.hour >= 13);
  if (morning.length >= 3 && afternoon.length >= 3) {
    const mPnl = morning.reduce((s, c) => s + c.pnl, 0);
    const aPnl = afternoon.reduce((s, c) => s + c.pnl, 0);
    if (Math.sign(mPnl) !== Math.sign(aPnl)) {
      const best = mPnl > aPnl ? 'le matin' : "l'après-midi";
      out.push({
        level: 'info',
        title: `Tu performes mieux ${best}`,
        detail: `Matin ${fmt(mPnl)} · après-midi ${fmt(aPnl)}. Concentre ton trading sur ta plage la plus rentable.`,
      });
    }
  }

  // --- Emotions (if tagged) ---
  const byEmotion = new Map<string, number>();
  for (const c of cs) if (c.emotion) byEmotion.set(c.emotion, (byEmotion.get(c.emotion) ?? 0) + c.pnl);
  if (byEmotion.size >= 2) {
    const sorted = [...byEmotion.entries()].sort((a, b) => a[1] - b[1]);
    const worst = sorted[0];
    if (worst[1] < 0) {
      out.push({
        level: 'warn',
        title: `Émotion la plus coûteuse : « ${worst[0]} »`,
        detail: `Tes trades tagués « ${worst[0]} » cumulent ${fmt(worst[1])}. Méfie-toi de cet état.`,
      });
    }
  }

  return out;
}
