// Neon equity curve: zoomed Y-axis with $ scale, gridlines, layered-stroke glow,
// and a period selector. Pure react-native-svg (no filters — glow is faked with
// stacked strokes, which is the reliable cross-platform approach).

import React from 'react';
import { StyleSheet, Text, View, useWindowDimensions } from 'react-native';
import Svg, { Circle, Defs, LinearGradient, Line, Path, Stop, Text as SvgText } from 'react-native-svg';

import { neon, moneyK } from '../../theme-neon';

export function NeonEquityChart({ values }: { values: number[] }) {
  const { width } = useWindowDimensions();
  const W = Math.max(240, width - 16 * 2 - 16 * 2);
  const H = 190;
  const padL = 46;
  const padR = 10;
  const padT = 14;
  const padB = 8;

  if (values.length < 2) {
    return (
      <View style={[styles.empty, { height: H }]}>
        <Text style={styles.emptyText}>Pas assez de trades pour la courbe.</Text>
      </View>
    );
  }

  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || Math.max(1, Math.abs(max) * 0.01);
  const lo = min - span * 0.35;
  const hi = max + span * 0.25;

  const x = (i: number) => padL + (i / (values.length - 1)) * (W - padL - padR);
  const y = (v: number) => padT + (1 - (v - lo) / (hi - lo)) * (H - padT - padB);

  const line = values.map((v, i) => `${i === 0 ? 'M' : 'L'} ${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join(' ');
  const area = `${line} L ${x(values.length - 1).toFixed(1)} ${y(lo).toFixed(1)} L ${x(0).toFixed(1)} ${y(lo).toFixed(1)} Z`;

  const ticks = 4;
  const gridY: { yy: number; label: string }[] = [];
  for (let k = 0; k <= ticks; k++) {
    const val = lo + (hi - lo) * (k / ticks);
    gridY.push({ yy: y(val), label: moneyK(val) });
  }

  const lastX = x(values.length - 1);
  const lastY = y(values[values.length - 1]);

  return (
    <Svg width={W} height={H}>
        <Defs>
          <LinearGradient id="neonFill" x1="0" y1="0" x2="0" y2="1">
            <Stop offset="0" stopColor={neon.green} stopOpacity={0.42} />
            <Stop offset="1" stopColor={neon.green} stopOpacity={0} />
          </LinearGradient>
        </Defs>

        {gridY.map((g, i) => (
          <React.Fragment key={i}>
            <Line x1={padL} y1={g.yy} x2={W - padR} y2={g.yy} stroke={neon.border} strokeWidth={1} />
            <SvgText x={padL - 6} y={g.yy + 3} fill={neon.muted} fontSize={9} textAnchor="end">
              {g.label}
            </SvgText>
          </React.Fragment>
        ))}

        <Path d={area} fill="url(#neonFill)" />
        {/* Layered strokes = glow without SVG filters. */}
        <Path d={line} fill="none" stroke={neon.green} strokeWidth={9} strokeOpacity={0.16} strokeLinejoin="round" strokeLinecap="round" />
        <Path d={line} fill="none" stroke={neon.green} strokeWidth={4.5} strokeOpacity={0.35} strokeLinejoin="round" strokeLinecap="round" />
        <Path d={line} fill="none" stroke={neon.green} strokeWidth={2.4} strokeLinejoin="round" strokeLinecap="round" />

        <Circle cx={lastX} cy={lastY} r={7} fill={neon.green} fillOpacity={0.25} />
        <Circle cx={lastX} cy={lastY} r={3.5} fill={neon.green} />
      </Svg>
  );
}

const styles = StyleSheet.create({
  empty: { alignItems: 'center', justifyContent: 'center' },
  emptyText: { color: neon.muted, fontSize: 13, textAlign: 'center' },
});
