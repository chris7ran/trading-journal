// AccuTrader-style dashboard blocks, all computed client-side from trades and
// interactive (tap a bar to see its detail). No native deps beyond react-native-svg.

import React, { useMemo, useState } from 'react';
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import Svg, { Line, Text as SvgText } from 'react-native-svg';

import type { Trade } from '../api/types';
import { colors, pnlColor, radius, spacing } from '../theme';
import { Section } from './Section';
import {
  healthSeries,
  setupPerformance,
  symbolPerformance,
  timeSlotPerformance,
} from '../utils/analytics';

function useCardWidth(): number {
  const { width } = useWindowDimensions();
  return Math.max(240, width - spacing.md * 2 - spacing.md * 2);
}

function fmtUsd(v: number): string {
  const sign = v < 0 ? '-' : '';
  const abs = Math.abs(Math.round(v));
  return `${sign}$${abs.toLocaleString('en-US')}`;
}

// --- Trading Health Trends --------------------------------------------------

const ZONES = [
  { v: 3, label: 'OutPerforming' },
  { v: 2, label: 'Good' },
  { v: 1, label: 'Caution' },
  { v: 0, label: 'Reassess' },
];

function zoneColor(v: number): string {
  if (v >= 2.25) return colors.greenBright;
  if (v >= 1.5) return colors.green;
  if (v >= 0.75) return colors.amber;
  return colors.red;
}

export function HealthTrendCard({ trades }: { trades: Trade[] }) {
  const series = useMemo(() => healthSeries(trades), [trades]);
  const W = useCardWidth();
  const H = 176;
  const GUT = 94;
  const padT = 12;
  const padB = 12;
  const padR = 8;

  const y = (v: number) => padT + (1 - v / 3) * (H - padT - padB);
  const x = (i: number, n: number) => GUT + (n <= 1 ? 0 : (i / (n - 1)) * (W - GUT - padR));

  return (
    <Section title="Trading Health Trends">
      <View style={styles.card}>
        {series.length < 2 ? (
          <Text style={styles.hint}>Pas assez de trades pour la tendance.</Text>
        ) : (
          <Svg width={W} height={H}>
            {ZONES.map((z) => (
              <React.Fragment key={z.label}>
                <Line
                  x1={GUT}
                  x2={W - padR}
                  y1={y(z.v)}
                  y2={y(z.v)}
                  stroke={colors.border}
                  strokeWidth={1}
                  strokeDasharray="4 5"
                />
                <SvgText x={4} y={y(z.v) + 4} fill={colors.textMuted} fontSize={11} fontWeight="600">
                  {z.label}
                </SvgText>
              </React.Fragment>
            ))}
            {series.slice(1).map((v, idx) => {
              const i = idx + 1;
              const prev = series[i - 1];
              return (
                <Line
                  key={i}
                  x1={x(i - 1, series.length)}
                  y1={y(prev)}
                  x2={x(i, series.length)}
                  y2={y(v)}
                  stroke={zoneColor((prev + v) / 2)}
                  strokeWidth={3}
                  strokeLinecap="round"
                />
              );
            })}
          </Svg>
        )}
      </View>
    </Section>
  );
}

// --- Performance by Day & Time ----------------------------------------------

const DAYS: Array<{ label: string; value: number | null }> = [
  { label: 'All', value: null },
  { label: 'Lun', value: 1 },
  { label: 'Mar', value: 2 },
  { label: 'Mer', value: 3 },
  { label: 'Jeu', value: 4 },
  { label: 'Ven', value: 5 },
  { label: 'Sam', value: 6 },
  { label: 'Dim', value: 0 },
];

export function DayTimeCard({ trades }: { trades: Trade[] }) {
  const [day, setDay] = useState<number | null>(null);
  const [sel, setSel] = useState<number | null>(null);
  const data = useMemo(() => timeSlotPerformance(trades, day), [trades, day]);

  const slots = data.slots;
  const maxAbs = Math.max(1, ...slots.map((s) => Math.abs(s.pnl)));
  const chosen = sel !== null && slots[sel] ? slots[sel] : null;

  const BARH = 150;
  const half = BARH / 2 - 6;
  const colW = 26;
  const barW = 12;

  return (
    <Section title="Performance by Day & Time">
      <ScrollView horizontal showsHorizontalScrollIndicator={false} style={styles.chips} contentContainerStyle={styles.chipsContent}>
        {DAYS.map((d, i) => {
          const active = d.value === day;
          return (
            <React.Fragment key={d.label}>
              <Pressable
                onPress={() => { setDay(d.value); setSel(null); }}
                style={[styles.chip, active && styles.chipActive]}
              >
                <Text style={[styles.chipText, active && styles.chipTextActive]}>{d.label}</Text>
              </Pressable>
              {i === 0 ? <View style={styles.chipSep} /> : null}
            </React.Fragment>
          );
        })}
      </ScrollView>

      <View style={styles.card}>
        <View style={styles.pillHead}>
          <Text style={styles.pillLabel}>Avg Profit </Text>
          <Text style={[styles.pillValue, { color: pnlColor(chosen ? chosen.avgPnl : data.avgPnl) }]}>
            {fmtUsd(chosen ? chosen.avgPnl : data.avgPnl)}
          </Text>
          <Text style={styles.pillLabel}>  Win Rate </Text>
          <Text style={[styles.pillValue, { color: colors.greenBright }]}>
            {((chosen ? chosen.winRate : data.winRate) * 100).toFixed(1)}%
          </Text>
          {chosen ? <Text style={styles.pillSlot}>{`\n${chosen.label}`}</Text> : null}
        </View>

        {slots.length === 0 ? (
          <Text style={styles.hint}>Aucun trade sur cette sélection.</Text>
        ) : (
          <ScrollView horizontal showsHorizontalScrollIndicator={false}>
            <View style={{ height: BARH, width: colW * slots.length }}>
              <View style={[styles.centerline, { top: BARH / 2 }]} />
              {slots.map((s, i) => {
                const up = s.pnl >= 0;
                const h = Math.max(4, (Math.abs(s.pnl) / maxAbs) * half);
                const dim = sel !== null && sel !== i;
                return (
                  <Pressable
                    key={s.startMin}
                    onPress={() => setSel((cur) => (cur === i ? null : i))}
                    style={{ position: 'absolute', left: i * colW, width: colW, height: BARH }}
                  >
                    <View
                      style={{
                        position: 'absolute',
                        left: colW / 2 - barW / 2,
                        width: barW,
                        height: h,
                        borderRadius: 4,
                        backgroundColor: up ? colors.greenBright : colors.redBright,
                        opacity: dim ? 0.35 : 1,
                        ...(up ? { bottom: BARH / 2 } : { top: BARH / 2 }),
                      }}
                    />
                  </Pressable>
                );
              })}
            </View>
          </ScrollView>
        )}
      </View>
    </Section>
  );
}

// --- Top Performing Setups --------------------------------------------------

export function SetupPerfCard({ trades }: { trades: Trade[] }) {
  const perf = useMemo(
    () => setupPerformance(trades).slice().sort((a, b) => b.winRate - a.winRate).slice(0, 6),
    [trades],
  );
  const [sel, setSel] = useState<number>(0);
  const chosen = perf[sel] ?? perf[0] ?? null;

  const BARH = 150;

  return (
    <Section title="Top Performing Setups">
      <View style={styles.card}>
        {perf.length === 0 ? (
          <Text style={styles.hint}>Assigne un setup à tes trades pour voir ce classement.</Text>
        ) : (
          <>
            {chosen ? (
              <View style={styles.setupHead}>
                <Text style={styles.setupHeadName} numberOfLines={1}>{chosen.name}</Text>
                <View style={styles.setupHeadMeta}>
                  <Text style={styles.setupHeadKv}>Win Rate <Text style={{ color: colors.greenBright }}>{(chosen.winRate * 100).toFixed(0)}%</Text></Text>
                  <Text style={styles.setupHeadKv}>Net <Text style={{ color: pnlColor(chosen.pnl) }}>{fmtUsd(chosen.pnl)}</Text></Text>
                </View>
              </View>
            ) : null}
            <View style={styles.barsRow}>
              {perf.map((p, i) => {
                const h = Math.max(6, p.winRate * (BARH - 28));
                const active = i === sel;
                return (
                  <Pressable key={p.name} style={styles.barCol} onPress={() => setSel(i)}>
                    <Text style={styles.barPct}>{(p.winRate * 100).toFixed(0)}%</Text>
                    <View
                      style={{
                        width: '70%',
                        height: h,
                        borderRadius: 6,
                        backgroundColor: active ? colors.greenBright : colors.green,
                        opacity: active ? 1 : 0.8,
                      }}
                    />
                    <Text style={styles.barName} numberOfLines={1}>{p.name}</Text>
                  </Pressable>
                );
              })}
            </View>
          </>
        )}
      </View>
    </Section>
  );
}

// --- Top Symbols Traded -----------------------------------------------------

export function TopSymbolsCard({ trades }: { trades: Trade[] }) {
  const syms = useMemo(() => symbolPerformance(trades).slice(0, 5), [trades]);
  if (syms.length === 0) return null;
  return (
    <Section title="Top Symbols Traded">
      <View style={styles.card}>
        {syms.map((s) => {
          const up = s.pnl >= 0;
          return (
            <View key={s.symbol} style={styles.symRow}>
              <Text style={[styles.symArrow, { color: up ? colors.green : colors.red }]}>{up ? '▲' : '▼'}</Text>
              <Text style={styles.symName}>{s.symbol}</Text>
              <Text style={styles.symKv}>Profit</Text>
              <Text style={[styles.symVal, { color: pnlColor(s.pnl) }]}>{fmtUsd(s.pnl)}</Text>
            </View>
          );
        })}
      </View>
    </Section>
  );
}

const styles = StyleSheet.create({
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm, marginHorizontal: spacing.md },
  rowHead: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', marginRight: spacing.md },
  card: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: radius.lg, padding: spacing.md, marginHorizontal: spacing.md },
  hint: { color: colors.textMuted, fontSize: 13, textAlign: 'center', paddingVertical: spacing.md },

  // day chips
  chips: { marginHorizontal: spacing.md, marginBottom: spacing.sm, flexGrow: 0 },
  chipsContent: { alignItems: 'center', gap: spacing.sm },
  chip: { paddingHorizontal: spacing.md, paddingVertical: spacing.xs + 2, borderRadius: radius.md },
  chipActive: { backgroundColor: colors.card2 },
  chipText: { color: colors.textMuted, fontSize: 14, fontWeight: '600' },
  chipTextActive: { color: colors.text },
  chipSep: { width: 1, height: 18, backgroundColor: colors.border, marginHorizontal: spacing.xs },

  // day/time pill + bars
  pillHead: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', alignSelf: 'flex-start', backgroundColor: colors.card2, borderRadius: radius.md, paddingHorizontal: spacing.md, paddingVertical: spacing.sm, marginBottom: spacing.md },
  pillLabel: { color: colors.text, fontSize: 14, fontWeight: '600' },
  pillValue: { fontSize: 14, fontWeight: '700' },
  pillSlot: { color: colors.text, fontSize: 13, width: '100%' },
  centerline: { position: 'absolute', left: 0, right: 0, height: 0, borderTopWidth: 2, borderColor: colors.greenBright, borderStyle: 'dashed' },

  // setup bars
  setupHead: { marginBottom: spacing.md },
  setupHeadName: { color: colors.text, fontSize: 15, fontWeight: '700' },
  setupHeadMeta: { flexDirection: 'row', gap: spacing.lg, marginTop: 2 },
  setupHeadKv: { color: colors.textMuted, fontSize: 13, fontWeight: '600' },
  barsRow: { flexDirection: 'row', alignItems: 'flex-end', justifyContent: 'space-around', height: 176 },
  barCol: { flex: 1, alignItems: 'center', justifyContent: 'flex-end', height: 176 },
  barPct: { color: colors.text, fontSize: 12, fontWeight: '600', marginBottom: 4 },
  barName: { color: colors.textMuted, fontSize: 10, marginTop: 6, maxWidth: '96%', textAlign: 'center' },

  // symbols
  symRow: { flexDirection: 'row', alignItems: 'center', paddingVertical: spacing.sm, borderBottomColor: colors.border, borderBottomWidth: StyleSheet.hairlineWidth },
  symArrow: { fontSize: 12, width: 18 },
  symName: { color: colors.text, fontSize: 15, fontWeight: '700', width: 80 },
  symKv: { color: colors.textMuted, fontSize: 13, flex: 1 },
  symVal: { fontSize: 15, fontWeight: '700' },
});
