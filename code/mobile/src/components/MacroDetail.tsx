// Bottom-sheet detail for a macro indicator: yearly history + variation, the
// matching upcoming release (estimate) from the calendar, and a sentiment read.

import React from 'react';
import { Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import type { EconIndicator, EcoEvent } from '../api/types';
import { colors, radius, spacing } from '../theme';
import { Sparkline } from './Charts';
import { indicatorYears, isMarket, macroCommentary, marketReflection, matchingEvent, variation } from '../utils/macro';

export function MacroDetail({
  indicator,
  events,
  onClose,
}: {
  indicator: EconIndicator | null;
  events: EcoEvent[];
  onClose: () => void;
}) {
  if (!indicator) return null;
  const ind = indicator;
  const v = variation(ind);
  const years = indicatorYears(ind);
  // Newest-first rows of {year, value, delta vs previous year}.
  const rows = years
    .map((yr, i) => ({ yr, val: ind.history[i], prev: i > 0 ? ind.history[i - 1] : null }))
    .reverse();
  const event = matchingEvent(ind, events);
  const commentary = macroCommentary(ind, event);
  const market = isMarket(ind);
  const monthly = ind.category === 'macro_monthly';
  const upColor = v && v.abs > 0 ? colors.greenBright : v && v.abs < 0 ? colors.redBright : colors.textMuted;
  const deltaSuffix = market ? 'vs séance préc.' : monthly ? 'vs mois préc.' : `vs ${Number(ind.year) - 1}`;
  const dec = ind.unit === '%' ? 2 : 1;

  return (
    <Modal visible transparent animationType="slide" onRequestClose={onClose}>
      <View style={styles.root}>
        <Pressable style={styles.backdropFill} onPress={onClose} />
        <View style={styles.sheet}>
          <View style={styles.handle} />
          <Pressable style={styles.closeX} onPress={onClose} hitSlop={10}>
            <Text style={styles.closeXText}>✕</Text>
          </Pressable>
          <ScrollView showsVerticalScrollIndicator={false} contentContainerStyle={{ paddingBottom: spacing.md }}>
            <Text style={styles.region}>{ind.region}</Text>
            <Text style={styles.label}>{ind.label}</Text>

            <View style={styles.headRow}>
              <Text style={styles.value}>{ind.value.toFixed(dec)}{ind.unit}</Text>
              {v ? (
                <Text style={[styles.delta, { color: upColor }]}>
                  {v.abs >= 0 ? '▲' : '▼'} {Math.abs(v.abs).toFixed(dec)}{ind.unit}
                  {v.pct != null ? ` (${v.abs >= 0 ? '+' : '-'}${Math.abs(v.pct).toFixed(1)}%)` : ''} {deltaSuffix}
                </Text>
              ) : null}
            </View>

            {ind.history.length > 1 ? (
              <View style={styles.spark}>
                <Sparkline values={ind.history} width={260} height={44} color={colors.primary2} />
              </View>
            ) : null}

            {event ? (
              <View style={styles.estBox}>
                <Text style={styles.estTitle}>Prochaine publication</Text>
                <Text style={styles.estName}>{event.title}</Text>
                <Text style={styles.estMeta}>{formatDate(event.date)}</Text>
                <View style={styles.estRow}>
                  <Text style={styles.estKv}>Estimé <Text style={styles.estStrong}>{event.forecast ?? '—'}</Text></Text>
                  <Text style={styles.estKv}>Mois dernier <Text style={styles.estStrong}>{event.previous ?? '—'}</Text></Text>
                </View>
              </View>
            ) : null}

            {market ? (
              <>
                <Text style={styles.section}>Lecture du marché</Text>
                <Text style={styles.sentiment}>{marketReflection(ind)}</Text>
              </>
            ) : (
              <>
                <Text style={styles.section}>Constat</Text>
                <Text style={styles.sentiment}>{commentary.constat}</Text>

                <Text style={styles.section}>Scénarios</Text>
                {commentary.scenarios.map((s) => (
                  <View key={s.label} style={styles.scenRow}>
                    <Text style={styles.scenLabel}>{s.label}</Text>
                    <Text style={styles.scenText}>{s.text}</Text>
                  </View>
                ))}
                <View style={styles.baseBox}>
                  <Text style={styles.baseTitle}>Lecture du marché</Text>
                  <Text style={styles.baseText}>{commentary.baseCase}</Text>
                </View>

                {!monthly ? (
                  <>
                    <Text style={styles.section}>Historique annuel</Text>
                    {rows.map((r) => {
                      const d = r.prev != null ? r.val - r.prev : null;
                      return (
                        <View key={r.yr} style={styles.histRow}>
                          <Text style={styles.histYear}>{r.yr}</Text>
                          <Text style={styles.histVal}>{r.val.toFixed(1)}{ind.unit}</Text>
                          <Text style={[styles.histDelta, { color: d == null ? colors.textMuted : d >= 0 ? colors.green : colors.red }]}>
                            {d == null ? '—' : `${d >= 0 ? '▲' : '▼'} ${Math.abs(d).toFixed(1)}`}
                          </Text>
                        </View>
                      );
                    })}
                  </>
                ) : null}
              </>
            )}

            <Text style={styles.disclaimer}>
              Macro annuel : World Bank. Macro mensuel US + Fed Funds : FRED. Marché (or, pétrole,
              taux) : Stooq/Yahoo, fin de journée. Estimé/mois dernier : calendrier Forex Factory.
            </Text>

            <Pressable style={styles.closeBtn} onPress={onClose}>
              <Text style={styles.closeText}>Fermer</Text>
            </Pressable>
          </ScrollView>
        </View>
      </View>
    </Modal>
  );
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleString('fr-FR', { weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' });
}

const styles = StyleSheet.create({
  root: { flex: 1, justifyContent: 'flex-end' },
  backdropFill: { ...StyleSheet.absoluteFillObject, backgroundColor: 'rgba(0,0,0,0.5)' },
  sheet: { backgroundColor: colors.surface, borderTopLeftRadius: radius.xl, borderTopRightRadius: radius.xl, padding: spacing.md, paddingBottom: spacing.lg, maxHeight: '88%' },
  handle: { alignSelf: 'center', width: 40, height: 4, borderRadius: 2, backgroundColor: colors.border, marginBottom: spacing.md },
  closeX: { position: 'absolute', right: spacing.sm, top: spacing.sm, zIndex: 10, width: 32, height: 32, borderRadius: 16, backgroundColor: colors.card2, alignItems: 'center', justifyContent: 'center' },
  closeXText: { color: colors.text, fontSize: 15, fontWeight: '700' },
  region: { color: colors.textMuted, fontSize: 12 },
  label: { color: colors.text, fontSize: 18, fontWeight: '700', marginTop: 2 },
  headRow: { flexDirection: 'row', alignItems: 'baseline', gap: spacing.md, marginTop: spacing.sm },
  value: { color: colors.text, fontSize: 32, fontWeight: '800' },
  delta: { fontSize: 13, fontWeight: '600', flexShrink: 1 },
  spark: { marginTop: spacing.sm, alignItems: 'flex-start' },
  estBox: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: radius.md, padding: spacing.md, marginTop: spacing.md },
  estTitle: { color: colors.textMuted, fontSize: 12 },
  estName: { color: colors.text, fontSize: 15, fontWeight: '600', marginTop: 2 },
  estMeta: { color: colors.textMuted, fontSize: 12, marginTop: 2 },
  estRow: { flexDirection: 'row', gap: spacing.lg, marginTop: spacing.sm },
  estKv: { color: colors.textMuted, fontSize: 13 },
  estStrong: { color: colors.text, fontWeight: '700' },
  section: { color: colors.text, fontSize: 15, fontWeight: '600', marginTop: spacing.lg, marginBottom: spacing.sm },
  sentiment: { color: colors.textMuted, fontSize: 14, lineHeight: 21 },
  scenRow: { marginBottom: spacing.sm },
  scenLabel: { color: colors.text, fontSize: 13, fontWeight: '700', marginBottom: 2 },
  scenText: { color: colors.textMuted, fontSize: 13, lineHeight: 19 },
  baseBox: { backgroundColor: colors.card2, borderRadius: radius.md, padding: spacing.md, marginTop: spacing.sm },
  baseTitle: { color: colors.text, fontSize: 13, fontWeight: '700', marginBottom: 4 },
  baseText: { color: colors.textMuted, fontSize: 13, lineHeight: 19 },
  histRow: { flexDirection: 'row', alignItems: 'center', paddingVertical: spacing.sm, borderBottomColor: colors.border, borderBottomWidth: StyleSheet.hairlineWidth },
  histYear: { color: colors.textMuted, fontSize: 14, width: 64 },
  histVal: { color: colors.text, fontSize: 15, fontWeight: '600', flex: 1 },
  histDelta: { fontSize: 13, fontWeight: '600' },
  disclaimer: { color: colors.textMuted, fontSize: 11, lineHeight: 16, marginTop: spacing.md },
  closeBtn: { backgroundColor: colors.card2, borderRadius: 12, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.md },
  closeText: { color: colors.text, fontSize: 15, fontWeight: '600' },
});
