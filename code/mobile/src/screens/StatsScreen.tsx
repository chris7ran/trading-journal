// Analytics: aggregated stats from GET /trades/stats.

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';

import type { Trade, TradeStats } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, pnlColor, spacing } from '../theme';
import { EquityCurve, PnlBars } from '../components/Charts';
import { CalendarHeatmap } from '../components/CalendarHeatmap';
import { FilterBar } from '../components/FilterBar';
import { dailyPnlSeries, equitySeries } from '../utils/series';
import { DEFAULT_FILTERS, Filters, filtersToQuery } from '../utils/filters';

export default function StatsScreen() {
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
      // Stats for the cards; trades to build the time-series charts.
      const q = filtersToQuery(filters);
      const [s, t] = await Promise.all([
        api.getStats(q),
        api.listTrades({ ...q, limit: 1000 }),
      ]);
      setStats(s);
      setTrades(t);
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

  const equity = useMemo(() => equitySeries(trades), [trades]);
  const daily = useMemo(() => dailyPnlSeries(trades), [trades]);

  useEffect(() => {
    load();
  }, [load]);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  const onRefresh = () => {
    setRefreshing(true);
    load();
  };

  return (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={
        <RefreshControl refreshing={refreshing} onRefresh={onRefresh} tintColor={colors.primary} />
      }
    >
      <FilterBar value={filters} symbols={allSymbols} onChange={setFilters} />

      {error ? <Text style={styles.error}>{error}</Text> : null}

      {stats ? (
        <>
          <View style={styles.grid}>
            <Stat label="Trades" value={String(stats.total_trades)} />
            <Stat label="Win rate" value={`${(stats.win_rate * 100).toFixed(1)}%`} />
            <Stat
              label="Profit factor"
              value={stats.profit_factor === null ? '—' : stats.profit_factor.toFixed(2)}
            />
            <Stat
              label="P&L total"
              value={formatPnl(stats.total_pnl)}
              color={pnlColor(stats.total_pnl)}
            />
            <Stat label="Gain moyen" value={formatPnl(stats.avg_win)} color={colors.green} />
            <Stat label="Perte moyenne" value={formatPnl(stats.avg_loss)} color={colors.red} />
          </View>

          <Text style={styles.sectionTitle}>Calendrier P&L</Text>
          <View style={styles.chartCard}>
            <CalendarHeatmap data={daily} />
          </View>

          <Text style={styles.sectionTitle}>Courbe d'equity (P&L cumulé)</Text>
          <View style={styles.chartCard}>
            <EquityCurve values={equity} />
          </View>

          <Text style={styles.sectionTitle}>P&L par jour</Text>
          <View style={styles.chartCard}>
            <PnlBars data={daily} />
          </View>

          <Text style={styles.sectionTitle}>Par instrument</Text>
          {stats.by_symbol.length === 0 ? (
            <Text style={styles.empty}>Aucune donnée.</Text>
          ) : (
            stats.by_symbol.map((s) => (
              <View key={s.symbol} style={styles.symbolRow}>
                <Text style={styles.symbol}>{s.symbol}</Text>
                <Text style={styles.symbolMeta}>
                  {s.trades} trades · {s.wins} gagnants
                </Text>
                <Text style={[styles.symbolPnl, { color: pnlColor(s.pnl) }]}>
                  {formatPnl(s.pnl)}
                </Text>
              </View>
            ))
          )}
        </>
      ) : null}
    </ScrollView>
  );
}

function Stat({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <View style={styles.card}>
      <Text style={styles.cardLabel}>{label}</Text>
      <Text style={[styles.cardValue, color ? { color } : null]}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  grid: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'space-between' },
  card: {
    backgroundColor: colors.card,
    borderRadius: 12,
    padding: spacing.md,
    width: '48%',
    marginBottom: spacing.sm + 4,
  },
  cardLabel: { color: colors.textMuted, fontSize: 13 },
  cardValue: { color: colors.text, fontSize: 22, fontWeight: '700', marginTop: spacing.xs },
  sectionTitle: {
    color: colors.text,
    fontSize: 17,
    fontWeight: '700',
    marginTop: spacing.md,
    marginBottom: spacing.sm,
  },
  chartCard: {
    backgroundColor: colors.card,
    borderRadius: 12,
    padding: spacing.md,
    marginBottom: spacing.sm,
  },
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
  error: { color: colors.red, marginBottom: spacing.md },
  empty: { color: colors.textMuted },
});
