// Lightweight SVG charts (no native deps — works in Expo Go via react-native-svg).

import React from 'react';
import { StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import Svg, { Defs, Line, LinearGradient, Path, Rect, Stop, Text as SvgText } from 'react-native-svg';

import { colors, gradients, spacing } from '../theme';
import type { DailyPnl } from '../utils/series';

const HEIGHT = 170;
const PAD = 10;
const GUTTER = 40; // left space for Y-axis labels

function fmtAxis(v: number): string {
  const a = Math.abs(v);
  if (a >= 1000) return `${(v / 1000).toFixed(1)}k`;
  return Math.round(v).toString();
}

// Chart width = screen minus the screen content padding and the card padding.
function useChartWidth(): number {
  const { width } = useWindowDimensions();
  return Math.max(220, width - spacing.md * 2 - spacing.md * 2);
}

function Empty({ label }: { label: string }) {
  return (
    <View style={[styles.empty, { height: HEIGHT }]}>
      <Text style={styles.emptyText}>{label}</Text>
    </View>
  );
}

/** Cumulative-P&L line + area chart. */
export function EquityCurve({ values }: { values: number[] }) {
  const W = useChartWidth();

  if (values.length < 2) {
    return <Empty label="Pas assez de trades pour la courbe d'equity." />;
  }

  // Include 0 so the baseline is always visible.
  const min = Math.min(0, ...values);
  const max = Math.max(0, ...values);
  const range = max - min || 1;

  const x = (i: number) => GUTTER + (i / (values.length - 1)) * (W - GUTTER - PAD);
  const y = (v: number) => PAD + (1 - (v - min) / range) * (HEIGHT - 2 * PAD);

  const line = values.map((v, i) => `${i === 0 ? 'M' : 'L'} ${x(i)} ${y(v)}`).join(' ');
  const area = `${line} L ${x(values.length - 1)} ${y(min)} L ${x(0)} ${y(min)} Z`;

  const zeroY = y(0);
  const showZero = min < 0 && max > 0;

  return (
    <Svg width={W} height={HEIGHT}>
      <Defs>
        <LinearGradient id="equityStroke" x1="0" y1="0" x2="1" y2="0">
          <Stop offset="0" stopColor={gradients.brand[0]} />
          <Stop offset="1" stopColor={gradients.brand[1]} />
        </LinearGradient>
        <LinearGradient id="equityFill" x1="0" y1="0" x2="0" y2="1">
          <Stop offset="0" stopColor={gradients.brand[0]} stopOpacity={0.35} />
          <Stop offset="1" stopColor={gradients.brand[0]} stopOpacity={0} />
        </LinearGradient>
      </Defs>
      {/* Y-axis scale */}
      <SvgText x={GUTTER - 4} y={y(max) + 3} fill={colors.textMuted} fontSize={9} textAnchor="end">{fmtAxis(max)}</SvgText>
      <SvgText x={GUTTER - 4} y={y(min) + 3} fill={colors.textMuted} fontSize={9} textAnchor="end">{fmtAxis(min)}</SvgText>
      {showZero ? (
        <SvgText x={GUTTER - 4} y={zeroY + 3} fill={colors.textMuted} fontSize={9} textAnchor="end">0</SvgText>
      ) : null}
      {/* zero baseline */}
      <Line x1={GUTTER} x2={W - PAD} y1={zeroY} y2={zeroY} stroke={colors.border} strokeWidth={1} strokeDasharray="4 4" />
      <Path d={area} fill="url(#equityFill)" />
      <Path d={line} fill="none" stroke="url(#equityStroke)" strokeWidth={2.2} />
    </Svg>
  );
}

/** Tiny inline sparkline for indicator cards. */
export function Sparkline({
  values,
  width = 80,
  height = 28,
  color = colors.primary2,
}: {
  values: number[];
  width?: number;
  height?: number;
  color?: string;
}) {
  if (values.length < 2) return <View style={{ width, height }} />;
  const p = 2;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const x = (i: number) => p + (i / (values.length - 1)) * (width - 2 * p);
  const y = (v: number) => p + (1 - (v - min) / range) * (height - 2 * p);
  const d = values.map((v, i) => `${i === 0 ? 'M' : 'L'} ${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join(' ');
  return (
    <Svg width={width} height={height}>
      <Path d={d} fill="none" stroke={color} strokeWidth={1.5} />
    </Svg>
  );
}

/** P&L per day as vertical bars, centered on a zero baseline. */
export function PnlBars({ data }: { data: DailyPnl[] }) {
  const W = useChartWidth();

  if (data.length === 0) {
    return <Empty label="Pas de données journalières." />;
  }

  const maxAbs = Math.max(1, ...data.map((d) => Math.abs(d.value)));
  const innerW = W - GUTTER - PAD;
  const innerH = HEIGHT - 2 * PAD;
  const zeroY = PAD + innerH / 2;
  const slot = innerW / data.length;
  const barW = Math.max(3, Math.min(26, slot * 0.62));

  return (
    <Svg width={W} height={HEIGHT}>
      {/* Y-axis scale */}
      <SvgText x={GUTTER - 4} y={PAD + 6} fill={colors.textMuted} fontSize={9} textAnchor="end">{`+${fmtAxis(maxAbs)}`}</SvgText>
      <SvgText x={GUTTER - 4} y={zeroY + 3} fill={colors.textMuted} fontSize={9} textAnchor="end">0</SvgText>
      <SvgText x={GUTTER - 4} y={HEIGHT - PAD} fill={colors.textMuted} fontSize={9} textAnchor="end">{`-${fmtAxis(maxAbs)}`}</SvgText>
      <Line x1={GUTTER} x2={W - PAD} y1={zeroY} y2={zeroY} stroke={colors.border} strokeWidth={1} />
      {data.map((d, i) => {
        const cx = GUTTER + slot * i + slot / 2;
        const h = (Math.abs(d.value) / maxAbs) * (innerH / 2 - 2);
        const yTop = d.value >= 0 ? zeroY - h : zeroY;
        return (
          <Rect
            key={d.day}
            x={cx - barW / 2}
            y={yTop}
            width={barW}
            height={Math.max(2, h)}
            rx={4}
            fill={d.value >= 0 ? colors.greenBright : colors.redBright}
          />
        );
      })}
    </Svg>
  );
}

const styles = StyleSheet.create({
  empty: { alignItems: 'center', justifyContent: 'center' },
  emptyText: { color: colors.textMuted, fontSize: 13, paddingHorizontal: spacing.md, textAlign: 'center' },
});
