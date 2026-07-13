// Monthly P&L calendar (TradeZella-style): one cell per day, tinted by P&L,
// with previous/next month navigation.

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';

import { colors, formatPnlCompact, spacing } from '../theme';
import type { DailyPnl } from '../utils/series';

const MONTHS_FR = [
  'Janvier', 'Février', 'Mars', 'Avril', 'Mai', 'Juin',
  'Juillet', 'Août', 'Septembre', 'Octobre', 'Novembre', 'Décembre',
];
const WEEKDAYS = ['L', 'M', 'M', 'J', 'V', 'S', 'D'];

interface ViewMonth {
  year: number;
  month: number; // 0-indexed
}

export function CalendarHeatmap({
  data,
  onDayPress,
}: {
  data: DailyPnl[];
  onDayPress?: (day: string) => void;
}) {
  // The month implied by the most recent data point (or today if empty).
  const initial = useMemo<ViewMonth>(() => {
    if (data.length > 0) {
      const k = data[data.length - 1].day;
      return { year: Number(k.slice(0, 4)), month: Number(k.slice(5, 7)) - 1 };
    }
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  }, [data]);

  const [view, setView] = useState<ViewMonth>(initial);
  const touched = useRef(false);

  // Follow the data's latest month until the user navigates manually.
  useEffect(() => {
    if (!touched.current) setView(initial);
  }, [initial]);

  const shift = (delta: number) => {
    touched.current = true;
    setView((v) => {
      let m = v.month + delta;
      let y = v.year;
      if (m < 0) { m = 11; y -= 1; }
      if (m > 11) { m = 0; y += 1; }
      return { year: y, month: m };
    });
  };

  const { year, month } = view;

  const byDay = new Map<number, number>();
  let monthTotal = 0;
  for (const d of data) {
    if (Number(d.day.slice(0, 4)) === year && Number(d.day.slice(5, 7)) - 1 === month) {
      byDay.set(Number(d.day.slice(8, 10)), d.value);
      monthTotal += d.value;
    }
  }

  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const firstWeekday = (new Date(year, month, 1).getDay() + 6) % 7; // Monday-first

  const slots: Array<number | null> = [];
  for (let i = 0; i < firstWeekday; i++) slots.push(null);
  for (let d = 1; d <= daysInMonth; d++) slots.push(d);
  while (slots.length % 7 !== 0) slots.push(null);

  return (
    <View>
      <View style={styles.header}>
        <Pressable onPress={() => shift(-1)} hitSlop={10} accessibilityLabel="Mois précédent">
          <Text style={styles.chevron}>‹</Text>
        </Pressable>
        <View style={styles.headerCenter}>
          <Text style={styles.month}>{MONTHS_FR[month]} {year}</Text>
          {byDay.size > 0 ? (
            <Text style={[styles.total, { color: monthTotal >= 0 ? colors.green : colors.red }]}>
              {formatPnlCompact(monthTotal)}
            </Text>
          ) : (
            <Text style={styles.totalMuted}>Aucun trade</Text>
          )}
        </View>
        <Pressable onPress={() => shift(1)} hitSlop={10} accessibilityLabel="Mois suivant">
          <Text style={styles.chevron}>›</Text>
        </Pressable>
      </View>

      <View style={styles.weekRow}>
        {WEEKDAYS.map((w, i) => (
          <Text key={i} style={styles.weekday}>{w}</Text>
        ))}
      </View>

      <View style={styles.grid}>
        {slots.map((day, i) => {
          if (day === null) return <View key={`b${i}`} style={styles.cell} />;
          const v = byDay.get(day);
          const has = v !== undefined;
          const positive = (v ?? 0) >= 0;
          const tint = !has ? colors.card2 : positive ? 'rgba(22,199,132,0.16)' : 'rgba(246,70,93,0.16)';
          const bc = !has ? colors.border : positive ? colors.green : colors.red;
          const inner = (
            <View style={[styles.dayBox, { backgroundColor: tint, borderColor: bc }]}>
              <Text style={styles.dayNum}>{day}</Text>
              {has ? (
                <Text style={[styles.dayPnl, { color: positive ? colors.green : colors.red }]}>
                  {formatPnlCompact(v)}
                </Text>
              ) : null}
            </View>
          );
          const dayKey = `${year}-${String(month + 1).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
          return (
            <View key={day} style={styles.cell}>
              {has && onDayPress ? (
                <Pressable style={styles.fill} onPress={() => onDayPress(dayKey)}>
                  {inner}
                </Pressable>
              ) : (
                inner
              )}
            </View>
          );
        })}
      </View>
    </View>
  );
}

const CELL = `${100 / 7}%`;

const styles = StyleSheet.create({
  header: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', marginBottom: spacing.sm },
  headerCenter: { alignItems: 'center' },
  chevron: { color: colors.primary2, fontSize: 26, fontWeight: '600', paddingHorizontal: spacing.sm },
  month: { color: colors.text, fontSize: 15, fontWeight: '600' },
  total: { fontSize: 13, fontWeight: '600', marginTop: 1 },
  totalMuted: { color: colors.textMuted, fontSize: 12, marginTop: 1 },
  weekRow: { flexDirection: 'row' },
  weekday: { width: CELL, textAlign: 'center', color: colors.textMuted, fontSize: 10, marginBottom: 2 },
  grid: { flexDirection: 'row', flexWrap: 'wrap' },
  cell: { width: CELL, aspectRatio: 1, padding: 2 },
  fill: { flex: 1 },
  dayBox: { flex: 1, borderWidth: 1, borderRadius: 7, alignItems: 'center', justifyContent: 'center' },
  dayNum: { color: colors.textMuted, fontSize: 10 },
  dayPnl: { fontSize: 9, fontWeight: '600', marginTop: 1 },
});
