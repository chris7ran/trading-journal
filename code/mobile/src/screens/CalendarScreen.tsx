// Economic calendar (red/orange impact), filterable by asset/currency.

import React, { useCallback, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';

import type { EcoEvent, EconIndicator } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';
import { AssetBar } from '../components/AssetBar';
import { Sparkline } from '../components/Charts';
import { MacroDetail } from '../components/MacroDetail';
import { Section } from '../components/Section';
import { Asset, matchEvent, matchIndicator } from '../utils/assets';
import { countdownLabel, indicatorHighlight } from '../utils/macro';

export default function CalendarScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [events, setEvents] = useState<EcoEvent[]>([]);
  const [econ, setEcon] = useState<EconIndicator[]>([]);
  const [asset, setAsset] = useState<Asset>('all');
  // Impact filter: high (red) + medium (orange) on by default, low (yellow) off.
  const [impacts, setImpacts] = useState<{ red: boolean; orange: boolean; yellow: boolean }>({
    red: true,
    orange: true,
    yellow: false,
  });
  const [selected, setSelected] = useState<EconIndicator | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      // Calendar + indicators independently; one failing doesn't blank the other.
      const [cal, ec] = await Promise.allSettled([api.getCalendar(), api.getEconomy()]);
      if (cal.status === 'fulfilled') {
        setEvents([...cal.value].sort((a, b) => a.date.localeCompare(b.date)));
      } else if (cal.reason instanceof ApiError && cal.reason.status === 401) {
        await signOut();
        return;
      } else {
        setError(cal.reason instanceof Error ? cal.reason.message : 'Calendrier injoignable.');
      }
      if (ec.status === 'fulfilled') setEcon(ec.value);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [api, signOut]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const filtered = useMemo(
    () => events.filter((e) => matchEvent(e, asset) && impacts[e.impact]),
    [events, asset, impacts],
  );

  // Next high-impact (red) events still to come, honoring the asset filter.
  const upcoming = useMemo(() => {
    const now = Date.now();
    return events
      .filter((e) => e.impact === 'red' && matchEvent(e, asset) && new Date(e.date).getTime() >= now)
      .slice(0, 3);
  }, [events, asset]);

  // Macro indicators filtered by asset, grouped by kind to avoid a huge row.
  const grouped = useMemo(() => {
    const list = econ.filter((i) => matchIndicator(i, asset));
    return {
      monthly: list.filter((i) => i.category === 'macro_monthly'),
      market: list.filter((i) => i.category === 'market'),
      annual: list.filter((i) => i.category === 'macro'),
    };
  }, [econ, asset]);
  const hasIndicators = grouped.monthly.length + grouped.market.length + grouped.annual.length > 0;

  const IMPACTS: { key: 'red' | 'orange' | 'yellow'; label: string; color: string }[] = [
    { key: 'red', label: 'Fort', color: colors.red },
    { key: 'orange', label: 'Moyen', color: colors.amber },
    { key: 'yellow', label: 'Faible', color: colors.textMuted },
  ];

  return (
    <View style={styles.container}>
      <AssetBar value={asset} onChange={setAsset} />
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

          {upcoming.length > 0 ? (
            <View style={styles.alertCard}>
              <Text style={styles.alertTitle}>⚠︎ À venir · fort impact</Text>
              {upcoming.map((e, i) => (
                <View key={`up-${i}`} style={styles.row}>
                  <View style={[styles.dot, { backgroundColor: colors.red }]} />
                  <View style={styles.body}>
                    <Text style={styles.title}>{e.title}</Text>
                    <Text style={styles.meta}>
                      {e.currency} · {formatDateTime(e.date)}
                      {e.forecast ? `  ·  prév. ${e.forecast}` : ''}
                    </Text>
                  </View>
                </View>
              ))}
            </View>
          ) : null}

          {hasIndicators ? (
            <Section title="Indicateurs macro" flush>
              <IndicatorRow title="Mensuel" items={grouped.monthly} events={events} onSelect={setSelected} />
              <IndicatorRow title="Marché" items={grouped.market} events={events} onSelect={setSelected} />
              <IndicatorRow title="Annuel" items={grouped.annual} events={events} onSelect={setSelected} />
            </Section>
          ) : null}

          <Section title="Calendrier éco · cette semaine" flush>
            <View style={styles.impactRow}>
              {IMPACTS.map((im) => {
                const on = impacts[im.key];
                return (
                  <Pressable
                    key={im.key}
                    style={[styles.impactChip, on ? styles.impactChipOn : { opacity: 0.45 }]}
                    onPress={() => setImpacts((p) => ({ ...p, [im.key]: !p[im.key] }))}
                  >
                    <View style={[styles.impactDot, { backgroundColor: im.color }]} />
                    <Text style={[styles.impactText, on && styles.impactTextOn]}>{im.label}</Text>
                  </Pressable>
                );
              })}
            </View>
            {filtered.length === 0 ? (
              <Text style={styles.empty}>Aucun événement.</Text>
            ) : (
              filtered.map((e, i) => (
                <View key={`${e.title}-${i}`} style={styles.row}>
                  <View style={[styles.dot, { backgroundColor: e.impact === 'red' ? colors.red : e.impact === 'orange' ? colors.amber : colors.textMuted }]} />
                  <View style={styles.body}>
                    <Text style={styles.title}>{e.title}</Text>
                    <Text style={styles.meta}>
                      {e.currency} · {formatDateTime(e.date)}
                      {e.forecast ? `  ·  prév. ${e.forecast}` : ''}
                      {e.previous ? `  ·  préc. ${e.previous}` : ''}
                    </Text>
                  </View>
                </View>
              ))
            )}
          </Section>
        </ScrollView>
      )}

      <MacroDetail indicator={selected} events={events} onClose={() => setSelected(null)} />
    </View>
  );
}

function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleString('fr-FR', { weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' });
}

/** A labeled horizontal row of indicator cards (one per kind: monthly/market/annual). */
function IndicatorRow({
  title,
  items,
  events,
  onSelect,
}: {
  title: string;
  items: EconIndicator[];
  events: EcoEvent[];
  onSelect: (ind: EconIndicator) => void;
}) {
  if (items.length === 0) return null;
  return (
    <>
      <Text style={styles.subLabel}>{title}</Text>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} style={styles.indBar} contentContainerStyle={styles.indRow}>
        {items.map((ind, i) => {
          const delta = ind.previous != null ? ind.value - ind.previous : null;
          const isMkt = ind.category === 'market';
          const isMonthly = ind.category === 'macro_monthly';
          const dec = ind.unit === '%' ? 2 : 1;
          const hl = indicatorHighlight(ind, events);
          const accent = hl ? (hl.strong ? colors.red : colors.amber) : null;
          return (
            <Pressable
              key={i}
              style={[styles.indCard, accent ? { borderColor: accent, borderWidth: 1.5 } : null]}
              onPress={() => onSelect(ind)}
            >
              {hl ? (
                <View style={[styles.hlBadge, { backgroundColor: accent as string }]}>
                  <Text style={styles.hlBadgeText}>📅 {countdownLabel(hl.days)}</Text>
                </View>
              ) : null}
              <Text style={styles.indRegion}>{ind.region}</Text>
              <Text style={styles.indLabel}>{ind.label}</Text>
              <Text style={styles.indValue}>
                {ind.value.toFixed(dec)}
                {ind.unit}
              </Text>
              <Sparkline values={ind.history} width={108} height={26} />
              <Text style={styles.indDelta}>
                {delta == null
                  ? `donnée ${ind.year}`
                  : isMkt
                    ? `${delta >= 0 ? '▲' : '▼'} ${Math.abs(delta).toFixed(dec)} · séance`
                    : isMonthly
                      ? `${delta >= 0 ? '▲' : '▼'} ${Math.abs(delta).toFixed(dec)} vs mois préc.`
                      : `${delta >= 0 ? '▲' : '▼'} ${Math.abs(delta).toFixed(1)} vs ${Number(ind.year) - 1}`}
              </Text>
            </Pressable>
          );
        })}
      </ScrollView>
    </>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  scroll: { flex: 1 },
  centered: { flex: 1, justifyContent: 'center' },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  error: { color: colors.red, marginBottom: spacing.sm },
  empty: { color: colors.textMuted, fontSize: 13 },
  section: { color: colors.text, fontSize: 15, fontWeight: '600', marginTop: spacing.sm, marginBottom: spacing.sm },
  subLabel: { color: colors.textMuted, fontSize: 12, fontWeight: '600', marginBottom: spacing.xs, textTransform: 'uppercase', letterSpacing: 0.4 },
  sectionHead: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  chevron: { color: colors.textMuted, fontSize: 16 },
  hlBadge: { position: 'absolute', top: 6, right: 6, borderRadius: 8, paddingHorizontal: 6, paddingVertical: 2, zIndex: 1 },
  hlBadgeText: { color: '#fff', fontSize: 10, fontWeight: '700' },
  alertCard: { backgroundColor: colors.card, borderColor: colors.red, borderWidth: 1, borderRadius: radius.md, padding: spacing.md, marginBottom: spacing.md },
  alertTitle: { color: colors.red, fontSize: 13, fontWeight: '700', marginBottom: spacing.sm },
  impactRow: { flexDirection: 'row', gap: spacing.sm, marginBottom: spacing.sm },
  impactChip: { flexDirection: 'row', alignItems: 'center', gap: 6, paddingHorizontal: spacing.md, paddingVertical: spacing.xs + 2, borderRadius: radius.lg, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.card },
  impactChipOn: { borderColor: colors.primary },
  impactDot: { width: 8, height: 8, borderRadius: 4 },
  impactText: { color: colors.textMuted, fontSize: 12, fontWeight: '600' },
  impactTextOn: { color: colors.text },
  indBar: { flexGrow: 0, marginBottom: spacing.sm },
  indRow: { gap: spacing.sm },
  indCard: {
    width: 132,
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.sm + 2,
  },
  indRegion: { color: colors.textMuted, fontSize: 10 },
  indLabel: { color: colors.text, fontSize: 12, fontWeight: '500', marginBottom: 2 },
  indValue: { color: colors.text, fontSize: 18, fontWeight: '700', marginBottom: 2 },
  indDelta: { color: colors.textMuted, fontSize: 10, marginTop: 2 },
  row: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  dot: { width: 8, height: 8, borderRadius: 4, marginTop: 5, marginRight: spacing.sm },
  body: { flex: 1 },
  title: { color: colors.text, fontSize: 14, fontWeight: '500' },
  meta: { color: colors.textMuted, fontSize: 12, marginTop: 2 },
});
