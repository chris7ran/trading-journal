// Coach: rule-based insights from the trade history (no external AI).

import React, { useCallback, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import { Ionicons } from '@expo/vector-icons';

import type { Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, pnlColor, radius, spacing } from '../theme';
import { AccountPicker } from '../components/AccountPicker';
import { FilterBar } from '../components/FilterBar';
import { Section } from '../components/Section';
import { computeInsights, InsightLevel } from '../utils/coach';
import { topMistakes, setupPerformance, emotionBreakdown } from '../utils/analytics';
import { DEFAULT_FILTERS, Filters, filtersToQuery } from '../utils/filters';

export default function CoachScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [trades, setTrades] = useState<Trade[]>([]);
  const [filters, setFilters] = useState<Filters>(DEFAULT_FILTERS);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const t = await api.listTrades({ ...filtersToQuery(filters), limit: 1000 });
      setTrades(t);
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

  const insights = useMemo(() => computeInsights(trades), [trades]);
  const mistakes = useMemo(() => topMistakes(trades), [trades]);
  const setupPerf = useMemo(() => setupPerformance(trades), [trades]);
  const emotionPerf = useMemo(() => emotionBreakdown(trades), [trades]);

  return (
    <View style={styles.outer}>
      <AccountPicker value={filters.accountId} onChange={(id) => setFilters((f) => ({ ...f, accountId: id }))} />
      <FilterBar value={filters} symbols={[]} onChange={setFilters} />
      {loading ? (
        <View style={styles.centered}>
          <ActivityIndicator color={colors.primary} size="large" />
        </View>
      ) : (
        <ScrollView
          style={styles.scroll}
          contentContainerStyle={styles.content}
          refreshControl={
            <RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />
          }
        >
          {error ? <Text style={styles.error}>{error}</Text> : null}
          <Text style={styles.intro}>Analyse automatique de tes trades — gratuite, calculée sur ton appareil.</Text>
          {insights.map((ins, i) => (
            <View key={i} style={[styles.card, { borderLeftColor: accentColor(ins.level) }]}>
              <View style={styles.cardHead}>
                <Ionicons name={icon(ins.level)} size={18} color={accentColor(ins.level)} />
                <Text style={styles.cardTitle}>{ins.title}</Text>
              </View>
              <Text style={styles.cardDetail}>{ins.detail}</Text>
            </View>
          ))}

          <Section flush title="Top erreurs à éviter">
            {mistakes.length === 0 ? (
              <Text style={styles.hint}>
                Renseigne la revue de tes trades (setup + questions) pour révéler tes erreurs récurrentes.
              </Text>
            ) : (
              mistakes.map((m) => (
                <View key={m.label} style={styles.mistakeRow}>
                  <View style={styles.mistakeBody}>
                    <Text style={styles.mLabel}>{m.label}</Text>
                    <Text style={styles.mSub}>{m.broken}× sur {m.reviewed} trade(s) noté(s)</Text>
                  </View>
                  <Text style={[styles.mPnl, { color: pnlColor(m.pnlImpact) }]}>{formatPnl(m.pnlImpact)}</Text>
                </View>
              ))
            )}
          </Section>

          <Section flush title="Performance par setup">
            {setupPerf.length === 0 ? (
              <Text style={styles.hint}>Assigne des setups à tes trades (carte de trade) pour comparer leur performance.</Text>
            ) : (
              setupPerf.map((p) => (
                <View key={p.name} style={styles.mistakeRow}>
                  <View style={styles.mistakeBody}>
                    <Text style={styles.mLabel}>{p.name}</Text>
                    <Text style={styles.mSub}>
                      {p.count} trades · {(p.winRate * 100).toFixed(0)}% win
                    </Text>
                  </View>
                  <Text style={[styles.mPnl, { color: pnlColor(p.pnl) }]}>{formatPnl(p.pnl)}</Text>
                </View>
              ))
            )}
          </Section>

          <Section flush title="Performance par émotion">
            {emotionPerf.length === 0 ? (
              <Text style={styles.hint}>Renseigne l'émotion de tes trades (peur, FOMO, discipline…) pour voir leur impact.</Text>
            ) : (
              emotionPerf.map((p) => (
                <View key={p.name} style={styles.mistakeRow}>
                  <View style={styles.mistakeBody}>
                    <Text style={styles.mLabel}>{p.name}</Text>
                    <Text style={styles.mSub}>
                      {p.count} trades · {(p.winRate * 100).toFixed(0)}% win
                    </Text>
                  </View>
                  <Text style={[styles.mPnl, { color: pnlColor(p.pnl) }]}>{formatPnl(p.pnl)}</Text>
                </View>
              ))
            )}
          </Section>
        </ScrollView>
      )}
    </View>
  );
}

function accentColor(level: InsightLevel): string {
  return level === 'good' ? colors.green : level === 'warn' ? colors.red : colors.primary2;
}

function icon(level: InsightLevel): keyof typeof Ionicons.glyphMap {
  return level === 'good' ? 'checkmark-circle-outline' : level === 'warn' ? 'alert-circle-outline' : 'bulb-outline';
}

const styles = StyleSheet.create({
  outer: { flex: 1, backgroundColor: colors.bg },
  scroll: { flex: 1 },
  centered: { flex: 1, justifyContent: 'center' },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  error: { color: colors.red, marginBottom: spacing.sm },
  intro: { color: colors.textMuted, fontSize: 12, marginBottom: spacing.md, lineHeight: 17 },
  card: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderLeftWidth: 3,
    borderRadius: radius.md,
    padding: spacing.md,
    marginBottom: spacing.sm + 2,
  },
  cardHead: { flexDirection: 'row', alignItems: 'center', gap: spacing.sm },
  cardTitle: { color: colors.text, fontSize: 15, fontWeight: '600', flex: 1 },
  cardDetail: { color: colors.textMuted, fontSize: 13, lineHeight: 19, marginTop: spacing.xs },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm },
  hint: { color: colors.textMuted, fontSize: 13, lineHeight: 18 },
  mistakeRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  mistakeBody: { flex: 1 },
  mLabel: { color: colors.text, fontSize: 14, fontWeight: '500' },
  mSub: { color: colors.textMuted, fontSize: 12, marginTop: 2 },
  mPnl: { fontSize: 14, fontWeight: '700' },
});
