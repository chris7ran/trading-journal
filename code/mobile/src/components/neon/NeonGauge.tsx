// Neon semicircle win-rate gauge: green arc (wins) + red arc (losses) over a
// muted track, with a glow (layered strokes) and the % + W/L count in the centre.

import React from 'react';
import Svg, { Path, Text as SvgText } from 'react-native-svg';

import { neon } from '../../theme-neon';

const W = 164;
const H = 96;
const R = 62;
const CX = 82;
const CY = 84;

function pt(a: number): [number, number] {
  return [CX + R * Math.cos(a), CY - R * Math.sin(a)];
}

function arc(fromA: number, toA: number): string {
  const [x0, y0] = pt(fromA);
  const [x1, y1] = pt(toA);
  return `M ${x0.toFixed(1)} ${y0.toFixed(1)} A ${R} ${R} 0 0 1 ${x1.toFixed(1)} ${y1.toFixed(1)}`;
}

export function NeonGauge({ ratio, wins, losses }: { ratio: number; wins: number; losses: number }) {
  const clamped = Math.max(0, Math.min(1, ratio));
  const splitA = Math.PI * (1 - clamped);
  const greenD = arc(Math.PI, splitA);
  const redD = arc(splitA, 0);
  const trackD = arc(Math.PI, 0);

  return (
    <Svg width={W} height={H}>
      <Path d={trackD} fill="none" stroke={neon.track} strokeWidth={11} strokeLinecap="round" />

      {/* glow layers */}
      <Path d={redD} fill="none" stroke={neon.red} strokeWidth={17} strokeOpacity={0.18} strokeLinecap="round" />
      <Path d={greenD} fill="none" stroke={neon.green} strokeWidth={17} strokeOpacity={0.2} strokeLinecap="round" />
      {/* solid arcs */}
      {clamped < 1 ? <Path d={redD} fill="none" stroke={neon.red} strokeWidth={11} strokeLinecap="round" /> : null}
      {clamped > 0 ? <Path d={greenD} fill="none" stroke={neon.green} strokeWidth={11} strokeLinecap="round" /> : null}

      <SvgText x={CX} y={68} fill={neon.text} fontSize={26} fontWeight="700" textAnchor="middle">
        {`${Math.round(clamped * 100)}%`}
      </SvgText>
      <SvgText x={CX} y={84} fill={neon.muted} fontSize={11} textAnchor="middle">
        {`${wins}W · ${losses}L`}
      </SvgText>
    </Svg>
  );
}
