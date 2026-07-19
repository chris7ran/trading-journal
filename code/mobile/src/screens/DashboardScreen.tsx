// Neon "web 3.0" dashboard, two tabs + a global period filter:
//   Account  = glance (balance, equity curve, KPIs, prop-firm)
//   Trading  = full analysis (gauge + RRR, avg win/loss, edge $, risk metrics,
//              health trends, day&time, setups, symbols, duration, type, daily P&L)
// The period selector (Tout / 30j / Trim. / Sem. / 1An) drives both tabs.

import React, { useCallback, useMemo, useState } from 'react';
import { ActivityIndicator, Pressable, RefreshControl, ScrollView, StyleSheet, Text, View } from 'react-native';
import { LinearGradient } from 'expo-linear-gradient';
import { useFocusEffect } from '@react-navigation/native';

import type { Account, Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { glow, money, moneySigned, neon } from '../theme-neon';
import {
  riskMetrics,
  setupPerformance,
  emotionBreakdown,
  durationBuckets,
  typeBreakdown,
} from '../utils/analytics';
import { dailyPnlSeries } from '../utils/series';
import { AccountPicker } from '../components/AccountPicker';
import { PropFirmCard } from '../components/PropFirmCard';
import { Section } from '../components/Section';
import { PnlBars } from '../components/Charts';
import { HealthTrendCard, DayTimeCard, SetupPerfCard, TopSymbolsCard } from '../components/DashboardExtras';
import { NeonEquityChart } from '../components/neon/NeonEquityChart';
import { NeonGauge } from '../components/neon/NeonGauge';

type Tab = 'account' | 'trading';
const PERIODS = ['Tout', '30j', 'Trim.', 'Sem.', '1An'] as const;
type Period = (typeof PERIODS)[number];
const PERIOD_DAYS: Record<Period, number> = { Tout: Infinity, '30j': 30, 'Trim.': 90, 'Sem.': 180, '1An': 365 };

export default function DashboardScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [accountId, setAccountId] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('account');
  const [period, setPeriod] = useState<Period>('Tout');

  const load = useCallback(async () => {
    setError(null);
    try {
      const q = accountId ? { account_id: accountId } : {};
      const [t, a] = await Promise.all([api.listTrades({ ...q, limit: 1000 }), api.listAccounts()]);
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

  const filtered = useMemo(() => filterByPeriod(trades, period), [trades, period]);
  const allStats = useMemo(() => computeStats(trades), [trades]); // all-time (hero)
  const s = useMemo(() => computeStats(filtered), [filtered]); // period (analysis)
  const rrr = s.avgLoss !== 0 ? s.avgWin / Math.abs(s.avgLoss) : null;
  const risk = useMemo(() => riskMetrics(filtered), [filtered]);
  const totalFees = useMemo(
    () => filtered.reduce((acc, t) => acc + (t.commission ?? 0) + (t.swap ?? 0), 0),
    [filtered],
  );
  const balanceSeries = useMemo(() => balanceSeries_(filtered, startBalance), [filtered, startBalance]);
  const daily = useMemo(() => dailyPnlSeries(filtered), [filtered]);
  const setupEdge = useMemo(() => setupPerformance(filtered).map((x) => ({ name: x.name, pnl: x.pnl })), [filtered]);
  const emotionEdge = useMemo(() => emotionBreakdown(filtered).map((x) => ({ name: x.name, pnl: x.pnl })), [filtered]);
  const sessionEdge = useMemo(() => sessionsByPnl(filtered), [filtered]);
  const durations = useMemo(() => durationBuckets(filtered), [filtered]);
  const types = useMemo(() => typeBreakdown(filtered), [filtered]);

  const equity = startBalance + allStats.total;
  const pnlPct = startBalance > 0 ? (allStats.total / startBalance) * 100 : 0;

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
        <TabButton label="Account" active={tab === 'account'} onPress={() => setTab('account')} />
        <TabButton label="Trading" active={tab === 'trading'} onPress={() => setTab('trading')} />
      </View>

      <View style={styles.periodBar}>
        {PERIODS.map((p) => {
          const on = p === period;
          return (
            <Pressable key={p} onPress={() => setPeriod(p)} style={[styles.periodPill, on && styles.periodPillOn]}>
              <Text style={[styles.periodText, on && styles.periodTextOn]}>{p}</Text>
            </Pressable>
          );
        })}
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
                <Text style={[styles.heroPnl, glowText(allStats.total >= 0 ? neon.green : neon.red)]}>
                  {moneySigned(allStats.total)}
                </Text>
                <Text style={[styles.pct, { color: allStats.total >= 0 ? neon.green : neon.red }]}>
                  {allStats.total >= 0 ? '▲' : '▼'} {Math.abs(pnlPct).toFixed(1)}%
                </Text>
              </View>
            </View>
            <View style={{ marginTop: 8 }}>
              <NeonEquityChart values={balanceSeries} />
            </View>
          </View>

          <View style={styles.row}>
            <Kpi label="Expectancy / trade" value={moneySigned(risk.expectancy)} color={risk.expectancy >= 0 ? neon.green : neon.red} glowIt />
            <Kpi label="Profit factor" value={s.pf === null ? '—' : s.pf.toFixed(2)} />
          </View>

          <Text style={styles.feesLine}>Frais (commission + swap) · {moneySigned(totalFees)}</Text>

          <PropFirmCard accountId={accountId} trades={trades} />
        </>
      ) : (
        <>
          <View style={[styles.card, glow(neon.green, 16, 0.2)]}>
            <View style={styles.gaugeRow}>
              <NeonGauge ratio={s.winRate} wins={s.wins} losses={s.losses} />
              <View style={{ flex: 1, marginLeft: 8 }}>
                <Text style={styles.lbl}>RRR moyen</Text>
                <Text style={styles.rrr}>{rrr === null ? '—' : rrr.toFixed(2)}</Text>
                <Text style={styles.note}>Le win rate seul est trompeur — lu avec le RRR.</Text>
              </View>
            </View>
          </View>

          <View style={styles.row}>
            <Kpi label="Avg win" value={moneySigned(s.avgWin)} color={neon.green} glowIt />
            <Kpi label="Avg loss" value={moneySigned(s.avgLoss)} color={neon.red} glowIt />
          </View>

          <Text style={styles.sectionLbl}>Où est ton edge</Text>
          <View style={[styles.card, { paddingVertical: 6 }]}>
            <EdgeRow label="Setup" list={setupEdge} />
            <EdgeRow label="Session" list={sessionEdge} />
            <EdgeRow label="Émotion" list={emotionEdge} />
            {setupEdge.length === 0 && emotionEdge.length === 0 && sessionEdge.length === 0 ? (
              <Text style={[styles.note, { paddingVertical: 10 }]}>Ajoute setups / émotions à tes trades pour révéler ton edge.</Text>
            ) : null}
          </View>

          {risk.count > 0 ? (
            <Section title="Risque & performance">
              <View style={styles.grid}>
                <GridMetric label="Gain espéré / trade" value={moneySigned(risk.expectancy)} color={risk.expectancy >= 0 ? neon.green : neon.red} />
                <GridMetric label="Max drawdown" value={moneySigned(risk.maxDrawdown)} color={neon.red} />
                <GridMetric label="Meilleur trade" value={moneySigned(risk.bestTrade)} color={neon.green} />
                <GridMetric label="Pire trade" value={moneySigned(risk.worstTrade)} color={neon.red} />
                <GridMetric label="Série de gains" value={String(risk.winStreak)} color={neon.green} />
                <GridMetric label="Série de pertes" value={String(risk.lossStreak)} color={neon.red} />
              </View>
            </Section>
          ) : null}

          <HealthTrendCard trades={filtered} />
          <DayTimeCard trades={filtered} />
          <SetupPerfCard trades={filtered} />
          <TopSymbolsCard trades={filtered} />

          {durations.length > 0 ? (
            <Section title="Performance par durée">
              <View style={styles.listCard}>
                {durations.map((b) => (
                  <ListRow key={b.label} name={b.label} meta={`${b.count} · ${(b.winRate * 100).toFixed(0)}%`} pnl={b.pnl} />
                ))}
              </View>
            </Section>
          ) : null}

          {types.length > 0 ? (
            <Section title="Type de trades">
              <View style={styles.listCard}>
                {types.map((b) => (
                  <ListRow key={b.label} name={b.label} meta={`${b.count} · ${(b.winRate * 100).toFixed(0)}%`} pnl={b.pnl} />
                ))}
              </View>
            </Section>
          ) : null}

          <Section title="P&L par jour">
            <View style={styles.chartCard}>
              <PnlBars data={daily} />
            </View>
          </Section>
        </>
      )}
    </ScrollView>
  );
}

function TabButton({ label, active, onPress }: { label: string; active: boolean; onPress: () => void }) {
  return (
    <Pressable style={styles.segBtn} onPress={onPress}>
      {active ? (
        <LinearGradient
          colors={[neon.green, neon.cyan]}
          start={{ x: 0, y: 0 }}
          end={{ x: 1, y: 0 }}
          style={[StyleSheet.absoluteFill, { borderRadius: 10 }]}
        />
      ) : null}
      <Text style={[styles.segText, active && styles.segTextOn]}>{label}</Text>
    </Pressable>
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

function GridMetric({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <View style={styles.gridMetric}>
      <Text style={styles.lbl}>{label}</Text>
      <Text style={[styles.kpiVal, color ? { color } : null]}>{value}</Text>
    </View>
  );
}

function ListRow({ name, meta, pnl }: { name: string; meta: string; pnl: number }) {
  return (
    <View style={styles.listRow}>
      <Text style={styles.listName} numberOfLines={1}>{name}</Text>
      <Text style={styles.listMeta}>{meta}</Text>
      <Text style={[styles.listPnl, { color: pnl >= 0 ? neon.green : neon.red }]}>{moneySigned(pnl)}</Text>
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

interface Stats {
  n: number;
  wins: number;
  losses: number;
  winRate: number;
  avgWin: number;
  avgLoss: number;
  pf: number | null;
  total: number;
}

function computeStats(trades: Trade[]): Stats {
  const closed = trades.filter((t) => t.pnl !== null && t.pnl !== undefined);
  let wins = 0;
  let losses = 0;
  let gw = 0;
  let gl = 0;
  let total = 0;
  for (const t of closed) {
    const p = t.pnl as number;
    total += p;
    if (p > 0) {
      wins += 1;
      gw += p;
    } else if (p < 0) {
      losses += 1;
      gl += -p;
    }
  }
  const n = closed.length;
  return {
    n,
    wins,
    losses,
    winRate: n ? wins / n : 0,
    avgWin: wins ? gw / wins : 0,
    avgLoss: losses ? -gl / losses : 0,
    pf: gl > 0 ? gw / gl : gw > 0 ? null : 0,
    total,
  };
}

function tradeTs(t: Trade): number | null {
  const raw = t.close_time ?? t.open_time ?? t.created_at;
  if (!raw) return null;
  const d = new Date(raw.replace(' ', 'T'));
  return isNaN(d.getTime()) ? null : d.getTime();
}

function filterByPeriod(trades: Trade[], period: Period): Trade[] {
  const days = PERIOD_DAYS[period];
  if (!isFinite(days)) return trades;
  const cutoff = Date.now() - days * 86400000;
  return trades.filter((t) => {
    const ts = tradeTs(t);
    return ts !== null && ts >= cutoff;
  });
}

function balanceSeries_(trades: Trade[], start: number): number[] {
  const pts = trades
    .filter((t) => t.pnl !== null && t.pnl !== undefined)
    .map((t) => ({ ts: tradeTs(t), pnl: t.pnl as number }))
    .filter((p): p is { ts: number; pnl: number } => p.ts !== null)
    .sort((a, b) => a.ts - b.ts);
  if (pts.length === 0) return [];
  let cum = start;
  return [
    start,
    ...pts.map((p) => {
      cum += p.pnl;
      return cum;
    }),
  ];
}

const SESSIONS: { name: string; lo: number; hi: number }[] = [
  { name: 'Asie', lo: 0, hi: 8 },
  { name: 'Londres', lo: 8, hi: 14 },
  { name: 'New York', lo: 14, hi: 22 },
  { name: 'Hors session', lo: 22, hi: 24 },
];

function sessionsByPnl(trades: Trade[]): { name: string; pnl: number }[] {
  const sums = new Map<string, number>();
  for (const t of trades) {
    if (t.pnl === null || t.pnl === undefined) continue;
    const ts = tradeTs(t);
    if (ts === null) continue;
    const h = new Date(ts).getHours();
    const sess = SESSIONS.find((x) => h >= x.lo && h < x.hi) ?? SESSIONS[3];
    sums.set(sess.name, (sums.get(sess.name) ?? 0) + (t.pnl as number));
  }
  return [...sums.entries()].map(([name, pnl]) => ({ name, pnl })).sort((a, b) => b.pnl - a.pnl);
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: neon.bg },
  content: { paddingBottom: 40 },
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
  segBtn: { flex: 1, alignItems: 'center', justifyContent: 'center', paddingVertical: 10, borderRadius: 10, overflow: 'hidden' },
  segText: { color: neon.muted, fontSize: 13, fontWeight: '600' },
  segTextOn: { color: '#04121A' },
  periodBar: { flexDirection: 'row', justifyContent: 'space-between', marginHorizontal: 16, marginTop: 10 },
  periodPill: { flex: 1, alignItems: 'center', paddingVertical: 7, marginHorizontal: 2, borderRadius: 9 },
  periodPillOn: { backgroundColor: 'rgba(34,211,238,0.12)', borderWidth: 1, borderColor: 'rgba(34,211,238,0.35)' },
  periodText: { color: neon.muted, fontSize: 12, fontWeight: '500' },
  periodTextOn: { color: neon.cyan },
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
  kpi: { flex: 1, backgroundColor: neon.panel, borderColor: neon.border, borderWidth: 1, borderRadius: 14, padding: 14 },
  kpiVal: { color: neon.text, fontSize: 18, fontWeight: '700', marginTop: 4 },
  feesLine: { color: neon.muted, fontSize: 12, marginHorizontal: 18, marginTop: 10 },
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
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: 12, marginHorizontal: 16 },
  gridMetric: { width: '47%', backgroundColor: neon.panel, borderColor: neon.border, borderWidth: 1, borderRadius: 14, padding: 14 },
  chartCard: { backgroundColor: neon.panel, borderColor: neon.border, borderWidth: 1, borderRadius: 14, padding: 12, marginHorizontal: 16 },
  listCard: { backgroundColor: neon.panel, borderColor: neon.border, borderWidth: 1, borderRadius: 14, paddingHorizontal: 14, marginHorizontal: 16 },
  listRow: { flexDirection: 'row', alignItems: 'center', paddingVertical: 11, borderBottomColor: neon.border, borderBottomWidth: StyleSheet.hairlineWidth },
  listName: { color: neon.text, fontSize: 14, fontWeight: '500', width: 110 },
  listMeta: { color: neon.muted, fontSize: 12, flex: 1 },
  listPnl: { fontSize: 14, fontWeight: '700' },
});
