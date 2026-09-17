// H1 candles with the day's levels drawn across them.
//
// The chart owns no market logic. It takes the very same `LadderRow[]` the
// screen already renders as a list, so a level cannot say one thing in the
// ladder and another on the chart — and the "tout afficher" toggle drives
// both at once.
//
// Two deliberate choices about the layout:
//
//   * **No left price axis.** On a phone it costs ~50px of plot to restate
//     numbers the level tags already show. The tags on the right, plus the
//     last-price marker, are the scale.
//   * **Index-spaced bars, not time-spaced.** Weekends and the nightly close
//     would otherwise open dead gaps a third of the chart wide.
//
// Pure react-native-svg, like the rest of the neon components — no WebView,
// no charting dependency.

import React, { useMemo, useRef, useState } from 'react';
import { Pressable, StyleSheet, Text, View, useWindowDimensions } from 'react-native';
import Svg, { Line, Rect, Text as SvgText } from 'react-native-svg';

import type { ChartBar } from '../../api/types';
import { neon } from '../../theme-neon';
import {
  KIND_COLOR,
  chartTag,
  formatPrice,
  humanDate,
  parisDate,
  parisTime,
  type LadderRow,
} from '../../utils/levels';

/** Visible windows, in bars. The payload holds a fortnight either way. */
const WINDOWS = [
  { key: '24h', label: '24 h', bars: 24 },
  { key: '2j', label: '2 j', bars: 48 },
  { key: '5j', label: '5 j', bars: 120 },
] as const;

type WindowKey = (typeof WINDOWS)[number]['key'];

/**
 * Which level gets a price tag when two of them land on the same pixel row.
 * The previous day's marks are the ones a decision is made against; the day's
 * own high and low are already visible as the edge of the candles.
 */
const TAG_PRIORITY: Record<LadderRow['kind'], number> = {
  previous: 0,
  orb: 1,
  session: 2,
  day: 3,
};

const H = 220;
const PAD_L = 6;
const PAD_R = 64;
const PAD_T = 16;
const PAD_B = 10;
/** Minimum vertical gap between two price tags, in pixels. */
const TAG_GAP = 11;

export function NeonLevelsChart({
  bars,
  levels,
  decimals,
}: {
  bars: ChartBar[];
  levels: LadderRow[];
  decimals: number;
}) {
  const { width } = useWindowDimensions();
  const [windowKey, setWindowKey] = useState<WindowKey>('2j');
  const [cursor, setCursor] = useState<number | null>(null);

  // Card margin (16) + card padding (16), both sides.
  const W = Math.max(240, width - 64);
  const plotL = PAD_L;
  const plotR = W - PAD_R;
  const plotW = plotR - plotL;

  const size = WINDOWS.find((w) => w.key === windowKey)?.bars ?? 48;
  const shown = useMemo(() => bars.slice(-size), [bars, size]);

  // Hooks must run unconditionally, so the geometry is computed even when
  // there is nothing to draw; the empty state is returned after.
  const geometry = useMemo(() => {
    if (shown.length === 0) return null;

    const cLo = Math.min(...shown.map((b) => b.low));
    const cHi = Math.max(...shown.map((b) => b.high));
    const cSpan = cHi - cLo || Math.max(Math.abs(cHi) * 0.001, 1e-6);

    // A level far outside the visible action would squash every candle into a
    // flat band to make room for it. Those stay off the chart — the ladder
    // below the chart still lists them.
    const band = cSpan * 0.45;
    const drawn = levels.filter((l) => l.price >= cLo - band && l.price <= cHi + band);

    const prices = [cLo, cHi, ...drawn.map((l) => l.price)];
    const lo = Math.min(...prices) - cSpan * 0.06;
    const hi = Math.max(...prices) + cSpan * 0.06;

    return { lo, hi, drawn };
  }, [shown, levels]);

  // The responder gives x in this View's coordinates; the slot width is what
  // turns that back into a bar.
  const slot = shown.length > 0 ? plotW / shown.length : plotW;
  const geom = useRef({ plotL, slot, count: shown.length });
  geom.current = { plotL, slot, count: shown.length };

  const pick = (x: number) => {
    const { plotL: l, slot: s, count } = geom.current;
    if (count === 0) return;
    const i = Math.round((x - l - s / 2) / s);
    setCursor(Math.max(0, Math.min(count - 1, i)));
  };

  const selector = (
    <View style={styles.windowBar}>
      {WINDOWS.map((w) => {
        const on = w.key === windowKey;
        return (
          <Pressable
            key={w.key}
            onPress={() => {
              setWindowKey(w.key);
              setCursor(null);
            }}
            style={[styles.windowBtn, on && styles.windowBtnOn]}
          >
            <Text style={[styles.windowText, on && styles.windowTextOn]}>{w.label}</Text>
          </Pressable>
        );
      })}
    </View>
  );

  if (!geometry || shown.length < 2) {
    return (
      <View>
        {selector}
        <View style={[styles.empty, { height: H }]}>
          <Text style={styles.emptyText}>Pas de bougies reçues pour cette date.</Text>
        </View>
      </View>
    );
  }

  const { lo, hi, drawn } = geometry;
  const y = (v: number) => PAD_T + (1 - (v - lo) / (hi - lo)) * (H - PAD_T - PAD_B);
  const cx = (i: number) => plotL + slot * (i + 0.5);
  const bodyW = Math.max(1.5, Math.min(9, slot * 0.66));

  const last = shown[shown.length - 1];
  const read = cursor !== null ? shown[cursor] : last;

  // Greedy tag placement: walk the levels by how much they matter and keep a
  // price tag only where nothing has been written yet. The line is always
  // drawn — it is the tag that collides, not the level.
  const taken: number[] = [y(last.close)];
  const tagged = new Set<string>();
  for (const level of [...drawn].sort((a, b) => TAG_PRIORITY[a.kind] - TAG_PRIORITY[b.kind])) {
    const yy = y(level.price);
    if (taken.some((t) => Math.abs(t - yy) < TAG_GAP)) continue;
    taken.push(yy);
    tagged.add(level.key);
  }

  // A vertical rule wherever the Paris day rolls over.
  const daySplits: { i: number; label: string }[] = [];
  for (let i = 1; i < shown.length; i++) {
    const day = parisDate(shown[i].ts);
    if (!day || day === parisDate(shown[i - 1].ts)) continue;
    const [weekday, dayNum] = humanDate(day).split(' ');
    daySplits.push({ i, label: `${weekday} ${dayNum}` });
  }

  const delta = read.close - read.open;
  const deltaColor = delta > 0 ? neon.green : delta < 0 ? neon.red : neon.muted;

  return (
    <View>
      {selector}

      {/* Readout: the hovered bar, or the latest one when nothing is held. */}
      <View style={styles.readout}>
        <Text style={styles.readTime}>
          {cursor !== null ? `${humanDate(parisDate(read.ts) ?? '')} · ` : 'Dernière · '}
          {parisTime(read.ts)}
        </Text>
        <Text style={styles.readPrice}>
          {formatPrice(read.close, decimals)}{' '}
          <Text style={[styles.readDelta, { color: deltaColor }]}>
            {delta >= 0 ? '+' : '−'}
            {formatPrice(Math.abs(delta), Math.min(decimals, 1))}
          </Text>
        </Text>
      </View>
      <Text style={styles.readOhlc}>
        H {formatPrice(read.high, decimals)} · B {formatPrice(read.low, decimals)}
        {read.bars < 12 ? ` · ${read.bars} bougies M5` : ''}
      </Text>

      <View
        onStartShouldSetResponder={() => true}
        onMoveShouldSetResponder={() => true}
        onResponderGrant={(e) => pick(e.nativeEvent.locationX)}
        onResponderMove={(e) => pick(e.nativeEvent.locationX)}
        onResponderRelease={() => setCursor(null)}
        onResponderTerminate={() => setCursor(null)}
      >
        <Svg width={W} height={H}>
          {/* Day boundaries, behind everything. */}
          {daySplits.map((d) => (
            <React.Fragment key={`split-${d.i}`}>
              <Line
                x1={plotL + slot * d.i}
                y1={PAD_T}
                x2={plotL + slot * d.i}
                y2={H - PAD_B}
                stroke={neon.border}
                strokeWidth={1}
              />
              <SvgText
                x={plotL + slot * d.i + 3}
                y={PAD_T - 5}
                fill={neon.muted}
                fontSize={8.5}
              >
                {d.label}
              </SvgText>
            </React.Fragment>
          ))}

          {/* Levels. Swept ones fade into the background: they are history. */}
          {drawn.map((l) => {
            const yy = y(l.price);
            const color = KIND_COLOR[l.kind];
            return (
              <React.Fragment key={l.key}>
                <Line
                  x1={plotL}
                  y1={yy}
                  x2={plotR}
                  y2={yy}
                  stroke={color}
                  strokeWidth={l.swept ? 1 : 1.3}
                  strokeOpacity={l.swept ? 0.3 : 0.75}
                  strokeDasharray={l.swept ? [2, 5] : [6, 4]}
                />
                <SvgText
                  x={plotL + 2}
                  y={yy - 3}
                  fill={color}
                  fillOpacity={l.swept ? 0.45 : 0.95}
                  fontSize={8.5}
                >
                  {chartTag(l)}
                  {l.swept ? ' ✓' : ''}
                </SvgText>
                {tagged.has(l.key) ? (
                  <SvgText
                    x={plotR + 5}
                    y={yy + 3}
                    fill={color}
                    fillOpacity={l.swept ? 0.5 : 1}
                    fontSize={9}
                  >
                    {formatPrice(l.price, decimals)}
                  </SvgText>
                ) : null}
              </React.Fragment>
            );
          })}

          {/* Candles. */}
          {shown.map((b, i) => {
            const up = b.close >= b.open;
            const color = up ? neon.green : neon.red;
            const x = cx(i);
            const top = Math.min(y(b.open), y(b.close));
            const height = Math.max(1, Math.abs(y(b.close) - y(b.open)));
            return (
              <React.Fragment key={b.ts}>
                <Line
                  x1={x}
                  y1={y(b.high)}
                  x2={x}
                  y2={y(b.low)}
                  stroke={color}
                  strokeWidth={1}
                  strokeOpacity={0.7}
                />
                <Rect
                  x={x - bodyW / 2}
                  y={top}
                  width={bodyW}
                  height={height}
                  fill={color}
                  fillOpacity={0.9}
                />
              </React.Fragment>
            );
          })}

          {/* Last price: the anchor that makes the whole scale readable. */}
          <Line
            x1={plotL}
            y1={y(last.close)}
            x2={plotR}
            y2={y(last.close)}
            stroke={neon.text}
            strokeWidth={1}
            strokeOpacity={0.5}
            strokeDasharray={[1, 3]}
          />
          <SvgText x={plotR + 5} y={y(last.close) + 3} fill={neon.text} fontSize={9} fontWeight="700">
            {formatPrice(last.close, decimals)}
          </SvgText>

          {/* Crosshair. */}
          {cursor !== null ? (
            <Line
              x1={cx(cursor)}
              y1={PAD_T}
              x2={cx(cursor)}
              y2={H - PAD_B}
              stroke={neon.cyan}
              strokeWidth={1}
              strokeOpacity={0.8}
            />
          ) : null}
        </Svg>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  windowBar: { flexDirection: 'row', gap: 6, marginTop: 10 },
  windowBtn: {
    paddingHorizontal: 12,
    paddingVertical: 5,
    borderRadius: 9,
    backgroundColor: neon.panel,
    borderWidth: 1,
    borderColor: neon.border,
  },
  windowBtnOn: { backgroundColor: 'rgba(34,211,238,0.14)', borderColor: 'rgba(34,211,238,0.35)' },
  windowText: { color: neon.muted, fontSize: 12, fontWeight: '600' },
  windowTextOn: { color: neon.cyan },

  readout: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'flex-end',
    marginTop: 12,
  },
  readTime: { color: neon.muted, fontSize: 11 },
  readPrice: { color: neon.text, fontSize: 16, fontWeight: '700' },
  readDelta: { fontSize: 12, fontWeight: '700' },
  readOhlc: { color: neon.muted, fontSize: 11, marginTop: 1 },

  empty: { alignItems: 'center', justifyContent: 'center' },
  emptyText: { color: neon.muted, fontSize: 13, textAlign: 'center' },
});
