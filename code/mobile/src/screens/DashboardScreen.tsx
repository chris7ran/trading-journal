// Neon "web 3.0" dashboard with two tabs:
//   Account  = quick glance (balance, equity curve, prop-firm, key KPIs)
//   Trading  = what to fix (win-rate gauge + RRR, avg win/loss, edge by $)
// Signal only — no vanity metrics. Data comes from the real analytics helpers.

import React, { useCallback, useMemo, useState } from 'react';
import { ActivityIndicator, Pressable, RefreshControl, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useFocusEffect } from '@react-navigation/native';

import type { Account, Trade, TradeStats } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { glow, money, moneySigned, neon } from '../theme-neon';
import { riskMetrics, setupPerformance, emotionBreakdown } from '../utils/analytics';
import { AccountPicker } from '../components/AccountPicker';
import { PropFirmCard } from '../components/PropFirmCard';
import { NeonEquityChart, type Period } from '../components/neon/NeonEquityChart';
import { NeonGauge } from '../components/neon/NeonGauge';

type Tab = 'account' | 'trading';

export default function DashboardScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [accountId, setAccountId] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [stats, setStats] = useState<TradeStats | null>(null);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('account');
  const [period, setPeriod] = useState<Period>('ALL');

  const load = useCallback(async () => {
    setError(null);
    try {
      const q = accountId ? { account_id: accountId } : {};
      const [s, t, a] = await Promise.all([
        api.getStats(q),
        api.listTrades({ ...q, limit: 1000 }),
        api.listAccounts(),
      ]);
      setStats(s);
      setTrades(t);
      setAccounts(a);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [api, signOut, accountId]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const account = useMemo(() => accounts.find((a) => a.id === accountId) ?? null, [accounts, accountId]);
  const startBalance = account?.balance ?? 100000;
  const total = stats?.total_pnl ?? 0;
  const equity = startBalance + total;
  const pnlPct = startBalance > 0 ? (total / startBalance) * 100 : 0;
  const wins = stats?.wins ?? 0;
  const losses = stats?.losses ?? 0;
  const winRate = stats?.win_rate ?? 0;
  const avgWin = stats?.avg_win ?? 0;
  const avgLoss = stats?.avg_loss ?? 0;
  const rrr = avgLoss !== 0 ? avgWin / Math.abs(avgLoss) : null;
  const pf = stats?.profit_factor ?? null;
  const expectancy = useMemo(() => riskMetrics(trades).expectancy, [trades]);

  const balanceSeries = useMemo(
    () => balanceSeriesForPeriod(trades, startBalance, period),
    [trades, startBalance, period],
  );

  const setupEdge = useMemo(() => setupPerformance(trades).map((s) => ({ name: s.name, pnl: s.pnl })), [trades]);
  const emotionEdge = useMemo(() => emotionBreakdown(trades).map((e) => ({ name: e.name, pnl: e.pnl })), [trades]);
  const sessionEdge = useMemo(() => sessionsByPnl(trades), [trades]);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={neon.green} size="large" />
      </View>
    );
  }

  return (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={
        <RefreshControl
          refreshing={refreshing}
          onRefresh={() => {
            setRefreshing(true);
            load();
          }}
          tintColor={neon.green}
        />
      }
    >
      <AccountPicker value={accountId} allowAll={false} onChange={setAccountId} />

      <View style={styles.seg}>
        <Pressable style={[styles.segBtn, tab === 'account' && styles.segBtnOn]} onPress={() => setTab('account')}>
          <Text style={[styles.segText, tab === 'account' && styles.segTextOn]}>Account</Text>
        </Pressable>
        <Pressable style={[styles.segBtn, tab === 'trading' && styles.segBtnOn]} onPress={() => setTab('trading')}>
          <Text style={[styles.segText, tab === 'trading' && styles.segTextOn]}>Trading</Text>
        </Pressable>
      </View>

      {error ? <Text style={styles.error}>{error}</Text> : null}

      {tab === 'account' ? (
        <>
          <View style={[styles.card, glow(neon.green, 18, 0.25)]}>
            <View style={styles.heroRow}>
              <View>
                <Text style={styles.lbl}>Balance</Text>
                <Text style={styles.hero}>{money(equity)}</Text>
              </View>
              <View style={{ alignItems: 'flex-end' }}>
                <Text style={styles.lbl}>Total P&amp;L</Text>
                <Text style={[styles.heroPnl, glowText(total >= 0 ? neon.green : neon.red)]}>{moneySigned(total)}</Text>
                <Text style={[styles.pct, { color: total >= 0 ? neon.green : neon.red }]}>
                  {total >= 0 ? '▲' : '▼'} {Math.abs(pnlPct).toFixed(1)}%
                </Text>
              </View>
            </View>
            <View style={{ marginTop: 8 }}>
              <NeonEquityChart values={balanceSeries} period={period} onPeriod={setPeriod} />
            </View>
          </View>

          <View style={styles.row}>
            <Kpi label="Expectancy / trade" value={moneySigned(expectancy)} color={expectancy >= 0 ? neon.green : neon.red} glowIt />
            <Kpi label="Profit factor" value={pf === null ? '—' : pf.toFixed(2)} />
          </View>

          <PropFirmCard accountId={accountId} trades={trades} />
        </>
      ) : (
        <>
          <View style={[styles.card, glow(neon.green, 16, 0.2)]}>
            <View style={styles.gaugeRow}>
              <NeonGauge ratio={winRate} wins={wins} losses={losses} />
              <View style={{ flex: 1, marginLeft: 8 }}>
                <Text style={styles.lbl}>RRR moyen</Text>
                <Text style={styles.rrr}>{rrr === null ? '—' : rrr.toFixed(2)}</Text>
                <Text style={styles.note}>Le win rate seul est trompeur — toujours lu avec le RRR.</Text>
              </View>
            </View>
          </View>

          <View style={styles.row}>
            <Kpi label="Avg win" value={moneySigned(avgWin)} color={neon.green} glowIt />
            <Kpi label="Avg loss" value={moneySigned(avgLoss)} color={neon.red} glowIt />
          </View>

          <Text style={styles.sectionLbl}>Où est ton edge</Text>
          <View style={[styles.card, { paddingVertical: 6 }]}>
            <EdgeRow label="Setup" list={setupEdge} />
            <EdgeRow label="Session" list={sessionEdge} />
            <EdgeRow label="Émotion" list={emotionEdge} />
            {setupEdge.length === 0 && emotionEdge.length === 0 && sessionEdge.length === 0 ? (
              <Text style={[styles.note, { paddingVertical: 10 }]}>Ajoute des setups / émotions à tes trades pour révéler ton edge.</Text>
            ) : null}
          </View>
        </>
      )}
    </ScrollView>
  );
}

function Kpi({ label, value, color, glowIt }: { label: string; value: string; color?: string; glowIt?: boolean }) {
  return (
    <View style={styles.kpi}>
      <Text style={styles.lbl}>{label}</Text>
      <Text style={[styles.kpiVal, color ? { color } : null, glowIt && color ? glowText(color) : null]}>{value}</Text>
    </View>
  );
}

function EdgeRow({ label, list }: { label: string; list: { name: string; pnl: number }[] }) {
  if (list.length === 0) return null;
  const best = list[0];
  const worst = list[list.length - 1];
  const showWorst = worst.name !== best.name;
  return (
    <View style={styles.edge}>
      <Text style={styles.edgeKey}>{label}</Text>
      <View style={styles.edgeVals}>
        <Text style={[styles.chip, { color: neon.green }]} numberOfLines={1}>
          {best.name} {edgeAmt(best.pnl)}
        </Text>
        {showWorst ? <Text style={styles.dot}>·</Text> : null}
        {showWorst ? (
          <Text style={[styles.chip, { color: neon.red }]} numberOfLines={1}>
            {worst.name} {edgeAmt(worst.pnl)}
          </Text>
        ) : null}
      </View>
    </View>
  );
}

// --- helpers ----------------------------------------------------------------

function glowText(color: string) {
  return { textShadowColor: color, textShadowRadius: 12, textShadowOffset: { width: 0, height: 0 } };
}

function edgeAmt(v: number): string {
  const s = v > 0 ? '+' : v < 0 ? '−' : '';
  const a = Math.abs(v);
  const str = a >= 1000 ? `${(a / 1000).toFixed(a >= 10000 ? 0 : 1)}k` : `${Math.round(a)}`;
  return `${s}$${str}`;
}

function tradeTs(t: Trade): number | null {
  const raw = t.close_time ?? t.open_time ?? t.created_at;
  if (!raw) return null;
  const d = new Date(raw.replace(' ', 'T'));
  return isNaN(d.getTime()) ? null : d.getTime();
}

const PERIOD_DAYS: Record<Period, number> = { '1D': 1, '1M': 30, '3M': 90, '6M': 180, '1Y': 365, ALL: Infinity };

/** Balance path (start balance + cumulative P&L), sliced to the chosen period. */
function balanceSeriesForPeriod(trades: Trade[], start: number, period: Period): number[] {
  const pts = trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => ({ ts: tradeTs(t), pnl: t.pnl as number }))
    .filter((p): p is { ts: number; pnl: number } => p.ts !== null)
    .sort((a, b) => a.ts - b.ts);
  if (pts.length === 0) return [];

  let cum = start;
  const path = pts.map((p) => {
    cum += p.pnl;
    return { ts: p.ts, bal: cum };
  });

  const days = PERIOD_DAYS[period];
  if (!isFinite(days)) return [start, ...path.map((p) => p.bal)];

  const cutoff = Date.now() - days * 86400000;
  let baseline = start;
  const win: number[] = [];
  for (const p of path) {
    if (p.ts < cutoff) baseline = p.bal;
    else win.push(p.bal);
  }
  if (win.length === 0) return [];
  return [baseline, ...win];
}

const SESSIONS: { name: string; lo: number; hi: number }[] = [
  { name: 'Asie', lo: 0, hi: 8 },
  { name: 'Londres', lo: 8, hi: 14 },
  { name: 'New York', lo: 14, hi: 22 },
  { name: 'Hors session', lo: 22, hi: 24 },
];

/** Cumulative P&L per trading session, best first. */
function sessionsByPnl(trades: Trade[]): { name: string; pnl: number }[] {
  const sums = new Map<string, number>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const ts = tradeTs(t);
    if (ts === null) continue;
    const h = new Date(ts).getHours();
    const s = SESSIONS.find((x) => h >= x.lo && h < x.hi) ?? SESSIONS[3];
    sums.set(s.name, (sums.get(s.name) ?? 0) + (t.pnl as number));
  }
  return [...sums.entries()].map(([name, pnl]) => ({ name, pnl })).sort((a, b) => b.pnl - a.pnl);
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: neon.bg },
  content: { paddingBottom: 32 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: neon.bg },
  error: { color: neon.red, paddingHorizontal: 16, paddingTop: 8 },
  seg: {
    flexDirection: 'row',
    gap: 6,
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 14,
    padding: 5,
    marginHorizontal: 16,
    marginTop: 10,
  },
  segBtn: { flex: 1, alignItems: 'center', paddingVertical: 10, borderRadius: 10 },
  segBtnOn: { backgroundColor: 'rgba(0,255,163,0.14)', borderWidth: 1, borderColor: 'rgba(0,255,163,0.4)' },
  segText: { color: neon.muted, fontSize: 13, fontWeight: '600' },
  segTextOn: { color: neon.green },
  card: {
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 18,
    padding: 16,
    marginHorizontal: 16,
    marginTop: 12,
  },
  heroRow: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'flex-start' },
  lbl: { color: neon.muted, fontSize: 11, letterSpacing: 0.4, textTransform: 'uppercase' },
  hero: { color: neon.text, fontSize: 30, fontWeight: '700', marginTop: 2 },
  heroPnl: { color: neon.green, fontSize: 20, fontWeight: '700', marginTop: 2 },
  pct: { fontSize: 12, marginTop: 2, fontWeight: '600' },
  row: { flexDirection: 'row', gap: 12, marginHorizontal: 16, marginTop: 12 },
  kpi: {
    flex: 1,
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 14,
    padding: 14,
  },
  kpiVal: { color: neon.text, fontSize: 18, fontWeight: '700', marginTop: 4 },
  gaugeRow: { flexDirection: 'row', alignItems: 'center' },
  rrr: { color: neon.text, fontSize: 26, fontWeight: '700', marginTop: 2 },
  note: { color: neon.muted, fontSize: 11, marginTop: 5, lineHeight: 16 },
  sectionLbl: { color: neon.muted, fontSize: 11, letterSpacing: 0.5, textTransform: 'uppercase', marginHorizontal: 18, marginTop: 18, marginBottom: 2 },
  edge: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingVertical: 11,
    borderBottomColor: neon.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  edgeKey: { color: neon.text, fontSize: 14, fontWeight: '500' },
  edgeVals: { flexDirection: 'row', alignItems: 'center', gap: 6, flexShrink: 1, marginLeft: 10 },
  chip: { fontSize: 12, fontWeight: '600', flexShrink: 1 },
  dot: { color: neon.muted, fontSize: 12 },
});
