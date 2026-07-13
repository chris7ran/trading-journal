// Home dashboard: gradient P&L header, discipline score, key metrics, and charts.

import React, { useCallback, useMemo, useState } from 'react';
import { ActivityIndicator, RefreshControl, ScrollView, StyleSheet, Text, View } from 'react-native';
import { LinearGradient } from 'expo-linear-gradient';
import { useFocusEffect } from '@react-navigation/native';

import type { Trade, TradeStats } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, gradients, pnlColor, radius, spacing } from '../theme';
import { ScoreGauge } from '../components/ScoreGauge';
import { EquityCurve, PnlBars } from '../components/Charts';
import { FilterBar } from '../components/FilterBar';
import { AccountPicker } from '../components/AccountPicker';
import { PropFirmCard } from '../components/PropFirmCard';
import { HealthTrendCard, DayTimeCard, SetupPerfCard, TopSymbolsCard } from '../components/DashboardExtras';
import { Section } from '../components/Section';
import { dailyPnlSeries, equitySeries } from '../utils/series';
import { computeScore, scoreLabel } from '../utils/score';
import { durationBuckets, riskMetrics, typeBreakdown } from '../utils/analytics';
import { DEFAULT_FILTERS, Filters, filtersToQuery } from '../utils/filters';

export default function DashboardScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [stats, setStats] = useState<TradeStats | null>(null);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState<Filters>(DEFAULT_FILTERS);
  const [allSymbols, setAllSymbols] = useState<string[]>([]);

  const load = useCallback(async () => {
    setError(null);
    try {
      const q = filtersToQuery(filters);
      const [s, t] = await Promise.all([
        api.getStats(q),
        api.listTrades({ ...q, limit: 1000 }),
      ]);
      setStats(s);
      setTrades(t);
      // Keep a stable, growing list of instruments for the filter chips.
      setAllSymbols((prev) => Array.from(new Set([...prev, ...t.map((x) => x.symbol)])).sort());
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
  }, [api, signOut, filters]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const equity = useMemo(() => equitySeries(trades), [trades]);
  const daily = useMemo(() => dailyPnlSeries(trades), [trades]);
  const score = useMemo(() => computeScore(stats), [stats]);
  const durations = useMemo(() => durationBuckets(trades), [trades]);
  const types = useMemo(() => typeBreakdown(trades), [trades]);
  const risk = useMemo(() => riskMetrics(trades), [trades]);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  const total = stats?.total_pnl ?? 0;
  const winPct = stats ? (stats.win_rate * 100).toFixed(1) : '0';
  const pf = stats?.profit_factor ?? null;

  return (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={<RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />}
    >
      <LinearGradient
        colors={[...gradients.brand]}
        start={{ x: 0, y: 0 }}
        end={{ x: 1, y: 1 }}
        style={styles.header}
      >
        <Text style={styles.headerLabel}>NET P&L</Text>
        <Text style={styles.headerValue}>{formatPnl(total)}</Text>
        <Text style={styles.headerSub}>
          {stats?.total_trades ?? 0} trades · {winPct}% win · PF {pf === null ? '—' : pf.toFixed(2)}
        </Text>
      </LinearGradient>

      <AccountPicker
        value={filters.accountId}
        onChange={(id) => setFilters((f) => ({ ...f, accountId: id }))}
      />
      <FilterBar value={filters} symbols={allSymbols} onChange={setFilters} />

      {error ? <Text style={styles.error}>{error}</Text> : null}

      <View style={styles.scoreCard}>
        <ScoreGauge score={score} size={76} />
        <View style={styles.scoreText}>
          <Text style={styles.scoreTitle}>Score · {scoreLabel(score)}</Text>
          <Text style={styles.scoreDesc}>Win rate, ratio gain/perte et profitabilité combinés.</Text>
        </View>
      </View>

      <View style={styles.grid}>
        <Metric label="Profit factor" value={pf === null ? '—' : pf.toFixed(2)} />
        <Metric label="Trades" value={String(stats?.total_trades ?? 0)} />
        <Metric label="Gain moyen" value={formatPnl(stats?.avg_win ?? 0)} color={colors.green} />
        <Metric label="Perte moyenne" value={formatPnl(stats?.avg_loss ?? 0)} color={colors.red} />
      </View>

      {risk.count > 0 ? (
        <Section title="Risque & performance">
          <View style={styles.grid}>
            <Metric label="Gain espéré / trade" value={formatPnl(risk.expectancy)} color={pnlColor(risk.expectancy)} />
            <Metric label="Max drawdown" value={formatPnl(risk.maxDrawdown)} color={colors.red} />
            <Metric label="Meilleur trade" value={formatPnl(risk.bestTrade)} color={colors.green} />
            <Metric label="Pire trade" value={formatPnl(risk.worstTrade)} color={colors.red} />
            <Metric label="Série de gains" value={`${risk.winStreak}`} color={colors.green} />
            <Metric label="Série de pertes" value={`${risk.lossStreak}`} color={colors.red} />
            <Metric label="Durée moyenne" value={formatHold(risk.avgHoldMin)} />
          </View>
        </Section>
      ) : null}

      <Section title="Courbe d'equity">
        <View style={styles.chartCard}>
          <EquityCurve values={equity} />
        </View>
      </Section>

      <Section title="P&L par jour">
        <View style={styles.chartCard}>
          <PnlBars data={daily} />
        </View>
      </Section>

      <HealthTrendCard trades={trades} />
      <DayTimeCard trades={trades} />
      <SetupPerfCard trades={trades} />
      <TopSymbolsCard trades={trades} />

      {durations.length > 0 ? (
        <Section title="Performance par durée">
          <View style={styles.chartCard}>
            {durations.map((b) => (
              <View key={b.label} style={styles.symbolRow}>
                <Text style={styles.symbol}>{b.label}</Text>
                <Text style={styles.symbolMeta}>
                  {b.count} trades · {(b.winRate * 100).toFixed(0)}% win
                </Text>
                <Text style={[styles.symbolPnl, { color: pnlColor(b.pnl) }]}>{formatPnl(b.pnl)}</Text>
              </View>
            ))}
          </View>
        </Section>
      ) : null}

      {types.length > 0 ? (
        <Section title="Type de trades">
          <View style={styles.chartCard}>
            {types.map((b) => (
              <View key={b.label} style={styles.symbolRow}>
                <Text style={styles.symbol}>{b.label}</Text>
                <Text style={styles.symbolMeta}>
                  {b.count} trades · {(b.winRate * 100).toFixed(0)}% win
                </Text>
                <Text style={[styles.symbolPnl, { color: pnlColor(b.pnl) }]}>{formatPnl(b.pnl)}</Text>
              </View>
            ))}
          </View>
        </Section>
      ) : null}

      <PropFirmCard accountId={filters.accountId} trades={trades} />
    </ScrollView>
  );
}

function formatHold(min: number | null): string {
  if (min == null) return '—';
  if (min < 60) return `${Math.round(min)} min`;
  if (min < 60 * 24) return `${(min / 60).toFixed(1)} h`;
  return `${(min / 1440).toFixed(1)} j`;
}

function Metric({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <View style={styles.metric}>
      <Text style={styles.metricLabel}>{label}</Text>
      <Text style={[styles.metricValue, color ? { color } : null]}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { paddingBottom: spacing.lg },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  header: {
    paddingTop: spacing.lg,
    paddingBottom: spacing.lg,
    paddingHorizontal: spacing.md,
    borderBottomLeftRadius: radius.xl,
    borderBottomRightRadius: radius.xl,
  },
  headerLabel: { color: '#E9E4FF', fontSize: 11, letterSpacing: 0.5 },
  headerValue: { color: '#FFFFFF', fontSize: 32, fontWeight: '700', marginTop: 2 },
  headerSub: { color: '#E9E4FF', fontSize: 12, marginTop: 4 },
  error: { color: colors.red, paddingHorizontal: spacing.md, paddingTop: spacing.sm },
  scoreCard: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.md,
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.lg,
    padding: spacing.md,
    margin: spacing.md,
  },
  scoreText: { flex: 1 },
  scoreTitle: { color: colors.text, fontSize: 14, fontWeight: '600' },
  scoreDesc: { color: colors.textMuted, fontSize: 12, marginTop: 3, lineHeight: 17 },
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: spacing.sm, paddingHorizontal: spacing.md },
  metric: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.md,
    width: '48%',
  },
  metricLabel: { color: colors.textMuted, fontSize: 12 },
  metricValue: { color: colors.text, fontSize: 20, fontWeight: '600', marginTop: 4 },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm, marginHorizontal: spacing.md },
  chartCard: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: radius.lg, padding: spacing.md, marginHorizontal: spacing.md },
  symbolRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  symbol: { color: colors.text, fontSize: 15, fontWeight: '600', width: 90 },
  symbolMeta: { color: colors.textMuted, fontSize: 13, flex: 1 },
  symbolPnl: { fontSize: 15, fontWeight: '700' },
});
