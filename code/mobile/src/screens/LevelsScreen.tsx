// "Niveaux" — two views behind one selector, no eighth tab in the bar.
//
// **Jour** — what you would mark on a chart before the open, ordered by how it
// gets used rather than by how the API returns it:
//   1. the daily bias against the midpoint of yesterday's range
//   2. the ADR guardrail: has the move already happened?
//   3. where the week stands
//   4. the ladder of levels, highest first, like a chart's price axis
//   5. what has already been taken, and the sessions themselves
//
// **Historique** — the archived briefs next to the trades actually taken. It
// exists so a broken archive gets noticed rather than filling up in silence,
// and so that after a few months the pattern is visible by scrolling.
//
// Everything is computed server-side from the M5 candles the MT5 Expert
// Advisor pushes. All times are shown in Paris time.

import React, { useCallback, useMemo, useRef, useState } from 'react';
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

import type {
  BiasRead,
  ChartBar,
  DailyBias,
  DailyBrief,
  LevelSymbol,
  LevelWindow,
  RangeStats,
  ReviewDay,
  WeeklyProfile,
} from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { glow, moneySigned, neon } from '../theme-neon';
import { NeonLevelsChart } from '../components/neon/NeonLevelsChart';
import {
  buildLadder,
  KIND_COLOR,
  formatPoints,
  formatPrice,
  humanDate,
  isEmpty,
  parisDate,
  parisTime,
  priceDecimals,
  shiftDate,
  todayParis,
  withData,
  type LadderRow,
} from '../utils/levels';

export default function LevelsScreen() {
  const api = useApi();
  const { signOut } = useAuth();

  const [symbols, setSymbols] = useState<LevelSymbol[]>([]);
  const [symbol, setSymbol] = useState<string | null>(null);
  const [date, setDate] = useState<string>(todayParis());
  const [brief, setBrief] = useState<DailyBrief | null>(null);
  const [bars, setBars] = useState<ChartBar[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [mode, setMode] = useState<'day' | 'history'>('day');
  const [review, setReview] = useState<ReviewDay[] | null>(null);

  // Opening the app on a Sunday should not show an empty screen. On the very
  // first load we jump to the last day the feed actually has data for; after
  // that the user's date choice always wins.
  const dateInitialised = useRef(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      // The picker only ever offers symbols the feed has delivered, so an
      // empty list means the EA has not run rather than a broken request.
      const available = await api.getLevelSymbols();
      setSymbols(available);

      const chosen =
        available.find((s) => s.symbol === symbol) ??
        available.find((s) => s.symbol === 'GER40') ??
        available[0];

      if (!chosen) {
        setBrief(null);
        return;
      }
      if (chosen.symbol !== symbol) setSymbol(chosen.symbol);

      let target = date;
      if (!dateInitialised.current) {
        dateInitialised.current = true;
        target = parisDate(chosen.last) ?? date;
        if (target !== date) setDate(target);
      }

      // Side by side: the chart is useless without the levels and the levels
      // read better with the chart, so neither waits on the other. A backend
      // that does not serve /candles yet still gets a working screen.
      const [nextBrief, nextBars] = await Promise.all([
        api.getBrief(chosen.symbol, target),
        api.getCandles(chosen.symbol, target, 'H1', 14).catch(() => [] as ChartBar[]),
      ]);
      setBrief(nextBrief);
      setBars(nextBars);

      // Only fetched when the history view is on screen: it walks two months
      // of snapshots and trades, and the day view never needs it.
      if (mode === 'history') {
        setReview(await api.getReview(chosen.symbol, shiftDate(todayParis(), -60)));
      }
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
  }, [api, signOut, symbol, date, mode]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const levels = brief?.levels ?? null;
  const decimals = useMemo(
    () => priceDecimals(levels?.day?.high ?? levels?.previous_day?.high ?? 1000),
    [levels],
  );
  const ladder = useMemo(() => (levels ? buildLadder(levels) : []), [levels]);
  const visible = expanded ? ladder : ladder.filter((r) => r.essential);
  const taken = useMemo(() => (levels?.sweeps ?? []).filter((s) => s.swept), [levels]);
  const sessions = useMemo(() => (levels ? withData(levels.sessions) : []), [levels]);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={neon.green} size="large" />
      </View>
    );
  }

  return (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={
        <RefreshControl
          refreshing={refreshing}
          onRefresh={() => {
            setRefreshing(true);
            load();
          }}
          tintColor={neon.green}
        />
      }
    >
      {/* Symbol picker */}
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={styles.symbolBar}
      >
        {symbols.map((s) => {
          const on = s.symbol === symbol;
          return (
            <Pressable
              key={s.symbol}
              // No spinner and no clearing: keeping the previous figures on
              // screen for the moment the request takes reads far better than
              // flashing an empty state.
              onPress={() => setSymbol(s.symbol)}
              style={[styles.symbolPill, on && styles.symbolPillOn]}
            >
              <Text style={[styles.symbolText, on && styles.symbolTextOn]}>{s.symbol}</Text>
            </Pressable>
          );
        })}
      </ScrollView>

      {/* Jour / Historique. Deliberately a selector inside this tab rather than
          an eighth tab in the bar — seven is already the practical limit. */}
      <View style={styles.modeBar}>
        {(['day', 'history'] as const).map((m) => {
          const on = m === mode;
          return (
            <Pressable
              key={m}
              onPress={() => setMode(m)}
              style={[styles.modeBtn, on && styles.modeBtnOn]}
            >
              <Text style={[styles.modeText, on && styles.modeTextOn]}>
                {m === 'day' ? 'Jour' : 'Historique'}
              </Text>
            </Pressable>
          );
        })}
      </View>

      {error ? <Text style={styles.error}>{error}</Text> : null}

      {mode === 'history' ? (
        <HistoryList
          days={review}
          onPick={(picked) => {
            setDate(picked);
            setMode('day');
          }}
        />
      ) : (
        <>
      {/* Date navigation */}
      <View style={styles.dateBar}>
        <Pressable onPress={() => setDate(shiftDate(date, -1))} hitSlop={12} style={styles.arrow}>
          <Text style={styles.arrowText}>‹</Text>
        </Pressable>

        <Pressable onPress={() => setDate(todayParis())}>
          <Text style={styles.dateText}>{humanDate(date)}</Text>
          {date !== todayParis() ? <Text style={styles.dateHint}>toucher pour revenir à aujourd'hui</Text> : null}
        </Pressable>

        <Pressable
          onPress={() => setDate(shiftDate(date, 1))}
          hitSlop={12}
          disabled={date >= todayParis()}
          style={styles.arrow}
        >
          <Text style={[styles.arrowText, date >= todayParis() && styles.arrowOff]}>›</Text>
        </Pressable>
      </View>

      {symbols.length === 0 ? (
        <Empty
          title="Aucune donnée de marché"
          body="L'Expert Advisor MT5 n'a encore rien envoyé. Vérifie qu'il tourne sur un graphique et que l'URL est autorisée dans Outils → Options → Expert Advisors."
        />
      ) : isEmpty(levels) ? (
        <Empty
          title="Pas de séance ce jour-là"
          body="Week-end, jour férié, ou données pas encore reçues pour cette date."
        />
      ) : (
        <>
          {/* The levels, drawn where they actually sit. Fed the same rows as
              the ladder below, so "tout afficher" expands both. */}
          <View style={[styles.card, glow(neon.cyan, 16, 0.12)]}>
            <Text style={styles.lbl}>Graphique H1 · niveaux</Text>
            <NeonLevelsChart bars={bars} levels={visible} decimals={decimals} />
          </View>

          {/* Context: is this move already big? */}
          {brief ? (
            <ContextCard
              stats={brief.stats}
              bias={brief.bias}
              weekly={brief.weekly}
              decimals={decimals}
            />
          ) : null}

          {/* Day range hero */}
          {levels?.day ? (
            <View style={[styles.card, glow(neon.cyan, 16, 0.18)]}>
              <Text style={styles.lbl}>Extrêmes du jour</Text>
              <Text style={styles.hero}>{formatPoints(levels.day.range, decimals)} pts</Text>
              <View style={styles.heroRow}>
                <View>
                  <Text style={styles.lbl}>Haut</Text>
                  <Text style={[styles.heroSide, { color: neon.green }]}>
                    {formatPrice(levels.day.high, decimals)}
                  </Text>
                  <Text style={styles.heroTime}>{parisTime(levels.day.high_at)}</Text>
                </View>
                <View style={{ alignItems: 'flex-end' }}>
                  <Text style={styles.lbl}>Bas</Text>
                  <Text style={[styles.heroSide, { color: neon.red }]}>
                    {formatPrice(levels.day.low, decimals)}
                  </Text>
                  <Text style={styles.heroTime}>{parisTime(levels.day.low_at)}</Text>
                </View>
              </View>
            </View>
          ) : null}

          {/* The ladder */}
          <Text style={styles.sectionLbl}>Échelle des niveaux</Text>
          <View style={styles.listCard}>
            {visible.map((row, i) => (
              <LadderLine
                key={row.key}
                row={row}
                decimals={decimals}
                previous={i > 0 ? visible[i - 1] : null}
                last={i === visible.length - 1}
              />
            ))}
          </View>

          {ladder.length > visible.length || expanded ? (
            <Pressable onPress={() => setExpanded(!expanded)} style={styles.moreBtn}>
              <Text style={styles.moreText}>
                {expanded
                  ? 'Afficher moins'
                  : `Tout afficher (${ladder.length - visible.length} de plus)`}
              </Text>
            </Pressable>
          ) : null}

          {/* Sweeps */}
          <Text style={styles.sectionLbl}>Liquidité prise</Text>
          <View style={styles.listCard}>
            {taken.length === 0 ? (
              <Text style={styles.emptyLine}>Aucun niveau n'a été pris ce jour-là.</Text>
            ) : (
              taken.map((s, i) => (
                <View key={s.key} style={[styles.sweepRow, i === taken.length - 1 && styles.lastRow]}>
                  <View style={{ flex: 1, paddingRight: 10 }}>
                    <Text style={styles.sweepLabel}>{s.label}</Text>
                    <Text style={styles.sweepMeta}>
                      {formatPrice(s.level, decimals)} · {parisTime(s.at)}
                    </Text>
                  </View>
                  <Text style={[styles.sweepExc, glowText(neon.green)]}>
                    {formatPoints(s.excursion, decimals)}
                  </Text>
                </View>
              ))
            )}
          </View>

          {/* Sessions recap */}
          <Text style={styles.sectionLbl}>Sessions</Text>
          <View style={styles.grid}>
            {sessions.map((s) => (
              <SessionCard key={s.key} window={s} decimals={decimals} />
            ))}
          </View>

          {/* Observations — the same facts, in words. This block is where an
              LLM-written version will slot in later, on top of these figures
              rather than instead of them. */}
          {brief && brief.observations.length > 0 ? (
            <>
              <Text style={styles.sectionLbl}>Constats</Text>
              <View style={styles.listCard}>
                {brief.observations.map((o, i) => (
                  <View
                    key={`${o.key}-${i}`}
                    style={[
                      styles.observationRow,
                      i === brief.observations.length - 1 && styles.lastRow,
                    ]}
                  >
                    <View style={styles.bullet} />
                    <Text style={styles.observationText}>{o.text}</Text>
                  </View>
                ))}
              </View>
            </>
          ) : null}

          <Text style={styles.footnote}>
            Calculé à partir des bougies M5 de ton broker. Sessions définies dans leur fuseau
            d'origine (Londres, New York, Tokyo), heures affichées en heure de Paris. Les
            pourcentages sont rapportés à l'ADR {brief?.stats.sessions ?? 20} séances : 100 % =
            une journée moyenne pour cet instrument.
          </Text>
        </>
      )}
        </>
      )}
    </ScrollView>
  );
}

/**
 * The review: what the brief said before each session, next to what was
 * actually traded that day.
 *
 * No verdict, no average. With a few weeks of history any statistic by bias
 * reading would be noise wearing the costume of insight — and the point of
 * showing this now is not to conclude, it is so that a broken archive gets
 * noticed instead of filling up in silence.
 */
function HistoryList({
  days,
  onPick,
}: {
  days: ReviewDay[] | null;
  onPick: (date: string) => void;
}) {
  if (days === null) {
    return (
      <View style={styles.centeredBlock}>
        <ActivityIndicator color={neon.green} />
      </View>
    );
  }
  if (days.length === 0) {
    return (
      <Empty
        title="Rien d'archivé pour l'instant"
        body="Les briefs sont figés automatiquement à 7h45 et 15h, du lundi au vendredi. Reviens après la première capture."
      />
    );
  }

  return (
    <>
      <Text style={styles.sectionLbl}>60 derniers jours</Text>
      <View style={styles.listCard}>
        {days.map((day, i) => (
          <ReviewRow
            key={day.date}
            day={day}
            last={i === days.length - 1}
            onPress={() => onPick(day.date)}
          />
        ))}
      </View>
      <Text style={styles.footnote}>
        Le biais affiché est celui qui était figé avant la séance, pas une relecture d'après coup.
        Les jours sans brief sont conservés : ce sont eux qui servent de point de comparaison.
      </Text>
    </>
  );
}

function ReviewRow({
  day,
  last,
  onPress,
}: {
  day: ReviewDay;
  last: boolean;
  onPress: () => void;
}) {
  // Two captures a day; the pre-London one is the morning read. Falling back to
  // whatever exists keeps manual and pre-NY captures visible too.
  const morning =
    day.snapshots.find((s) => s.session === 'pre_london') ?? day.snapshots[0] ?? null;
  const read = morning?.bias_read ?? null;
  const tally = day.trades_symbol.count > 0 ? day.trades_symbol : day.trades_all;

  return (
    <Pressable onPress={onPress} style={[styles.reviewRow, last && styles.lastRow]}>
      <View style={{ flex: 1, paddingRight: 10 }}>
        <Text style={styles.reviewDate}>{humanDate(day.date)}</Text>
        <Text style={[styles.reviewBias, { color: read ? biasColour(read) : neon.muted }]}>
          {read ? biasLabel(read) : 'Pas de brief'}
        </Text>
        {day.snapshots.length > 1 ? (
          <Text style={styles.reviewSessions}>{day.snapshots.length} captures</Text>
        ) : null}
      </View>

      <View style={{ alignItems: 'flex-end' }}>
        <Text style={styles.reviewTrades}>
          {tally.count === 0 ? 'aucun trade' : `${tally.count} trade${tally.count > 1 ? 's' : ''}`}
        </Text>
        {tally.count > 0 ? (
          <Text style={[styles.reviewPnl, { color: tally.pnl >= 0 ? neon.green : neon.red }]}>
            {moneySigned(tally.pnl)}
          </Text>
        ) : null}
      </View>
    </Pressable>
  );
}

/**
 * Two questions, in the order they get asked in the morning.
 *
 * First the bias: where does price sit against the midpoint of yesterday's
 * range? Second the guardrail: has the move already happened? A range in points
 * is meaningless without a yardstick — 250 points is a quiet day on one
 * instrument and an outlier on another — so everything is a percentage of that
 * instrument's own average daily range, with 100% marked on the bar.
 */
function ContextCard({
  stats,
  bias,
  weekly,
  decimals,
}: {
  stats: RangeStats;
  bias: DailyBias;
  weekly: WeeklyProfile;
  decimals: number;
}) {
  return (
    <>
      <BiasCard bias={bias} decimals={decimals} />
      <AdrCard stats={stats} decimals={decimals} />
      <WeeklyCard weekly={weekly} decimals={decimals} />
    </>
  );
}

function BiasCard({ bias, decimals }: { bias: DailyBias; decimals: number }) {
  if (bias.previous_mid === null) {
    return (
      <View style={styles.card}>
        <Text style={styles.lbl}>Biais daily</Text>
        <Text style={styles.emptyBody}>
          Pas de séance précédente exploitable pour cette date, donc pas de 0,5 de référence.
        </Text>
      </View>
    );
  }

  const colour = biasColour(bias.read);

  return (
    <View style={[styles.card, glow(colour, 16, 0.2)]}>
      <Text style={styles.lbl}>Biais daily · 0,5 de la veille</Text>
      <Text style={styles.biasMid}>{formatPrice(bias.previous_mid, decimals)}</Text>

      <Text style={[styles.biasRead, { color: colour }]}>{biasLabel(bias.read)}</Text>

      {/* The one fact available before the session. Marked as such, because
          everything below it is hindsight and must not be read as a forecast. */}
      {bias.open_above_mid !== null ? (
        <View style={styles.biasOpenRow}>
          <View style={[styles.tick, { backgroundColor: colour, height: 16 }]} />
          <Text style={styles.biasOpenText}>
            Ouverture {bias.open_above_mid ? 'au-dessus' : 'en dessous'} du 0,5
            <Text style={styles.biasOpenHint}> · connu avant la séance</Text>
          </Text>
        </View>
      ) : null}

      <View style={styles.contextDivider} />

      <View style={styles.flagGrid}>
        <Flag label="Bas tenu > 0,5" value={bias.held_above_mid} />
        <Flag label="Clôture > 0,5" value={bias.closed_above_mid} />
        <Flag label="PDH pris" value={bias.touched_pdh} />
        <Flag label="PDL pris" value={bias.touched_pdl} />
      </View>

      {bias.first_taken ? (
        <Text style={styles.contextMeta}>Premier extrême atteint : {bias.first_taken}</Text>
      ) : null}
      <Text style={styles.biasFootnote}>
        Ces quatre marqueurs se constatent à la clôture. Ils servent au débrief, pas à décider le
        matin.
      </Text>
    </View>
  );
}

function AdrCard({ stats, decimals }: { stats: RangeStats; decimals: number }) {
  // Under five completed sessions the backend withholds the averages rather
  // than computing a misleading one, so there is nothing to show.
  if (stats.adr === null || stats.day_pct_of_adr === null) {
    return (
      <View style={styles.card}>
        <Text style={styles.lbl}>Amplitude du jour</Text>
        <Text style={styles.emptyBody}>
          Pas encore assez d'historique pour calculer l'ADR ({stats.sessions} séance
          {stats.sessions > 1 ? 's' : ''} sur les 5 minimum). Laisse l'EA tourner quelques jours.
        </Text>
      </View>
    );
  }

  const pct = stats.day_pct_of_adr;
  const colour = adrColour(pct);

  return (
    <View style={[styles.card, glow(colour, 16, 0.2)]}>
      <Text style={styles.lbl}>Amplitude du jour</Text>

      <View style={styles.adrRow}>
        <Text style={[styles.adrValue, { color: colour }]}>{Math.round(pct)} %</Text>
        <Text style={styles.adrCaption}>de l'ADR {stats.sessions} séances</Text>
      </View>

      <AdrBar pct={pct} colour={colour} />

      <Text style={styles.adrDetail}>
        {formatPrice(stats.day_range ?? 0, decimals)} pts parcourus · moyenne{' '}
        {formatPrice(stats.adr, decimals)} · médiane {formatPrice(stats.adr_median ?? 0, decimals)}
      </Text>

      <View style={styles.contextDivider} />

      <View style={styles.contextRow}>
        <MiniStat label="Veille" pct={stats.previous_day_pct_of_adr} suffix="d'ADR" align="flex-start" />
        <MiniStat label="Range Asie" pct={stats.asia_pct_of_median} suffix="de sa médiane" />
      </View>
    </View>
  );
}

function WeeklyCard({ weekly, decimals }: { weekly: WeeklyProfile; decimals: number }) {
  if (!weekly.high_weekday || !weekly.low_weekday) {
    return null;
  }
  return (
    <View style={styles.card}>
      <Text style={styles.lbl}>
        Semaine en cours · {weekly.sessions} séance{weekly.sessions > 1 ? 's' : ''} · on est{' '}
        {weekly.today_weekday}
      </Text>

      <View style={styles.weekRow}>
        <View style={{ flex: 1 }}>
          <Text style={styles.weekDay}>{weekly.high_weekday}</Text>
          <Text style={[styles.weekValue, { color: neon.green }]}>
            {formatPrice(weekly.high ?? 0, decimals)}
          </Text>
          <Text style={styles.miniSuffix}>haut de la semaine</Text>
        </View>
        <View style={{ flex: 1, alignItems: 'flex-end' }}>
          <Text style={styles.weekDay}>{weekly.low_weekday}</Text>
          <Text style={[styles.weekValue, { color: neon.red }]}>
            {formatPrice(weekly.low ?? 0, decimals)}
          </Text>
          <Text style={styles.miniSuffix}>bas de la semaine</Text>
        </View>
      </View>

      {weekly.range !== null ? (
        <Text style={styles.adrDetail}>
          Amplitude {formatPrice(weekly.range, decimals)} pts
          {weekly.range_pct_of_adr !== null
            ? ` · ${Math.round(weekly.range_pct_of_adr)} % d'une journée moyenne`
            : ''}
        </Text>
      ) : null}
    </View>
  );
}

/** A yes/no marker. Grey when the backend could not establish it. */
function Flag({ label, value }: { label: string; value: boolean | null }) {
  const colour = value === null ? neon.muted : value ? neon.green : neon.muted;
  return (
    <View style={styles.flag}>
      <Text style={[styles.flagMark, { color: colour }]}>
        {value === null ? '—' : value ? '✓' : '·'}
      </Text>
      <Text style={styles.flagLabel}>{label}</Text>
    </View>
  );
}

/** Horizontal bar with a tick at 100%, clamped so an outlier stays readable. */
function AdrBar({ pct, colour }: { pct: number; colour: string }) {
  const CAP = 200; // beyond twice the average the exact width stops mattering
  const width = Math.max(2, (Math.min(pct, CAP) / CAP) * 100);

  return (
    <View style={styles.barTrack}>
      <View style={[styles.barFill, { width: `${width}%`, backgroundColor: colour }]} />
      {/* 100% sits at half the track since the cap is 200%. */}
      <View style={styles.barTick} />
    </View>
  );
}

function MiniStat({
  label,
  pct,
  suffix,
  align = 'flex-end',
}: {
  label: string;
  pct: number | null;
  suffix: string;
  align?: 'flex-start' | 'flex-end';
}) {
  return (
    <View style={{ alignItems: align, flex: 1 }}>
      <Text style={styles.lbl}>{label}</Text>
      <Text style={[styles.miniValue, { color: pct === null ? neon.muted : adrColour(pct) }]}>
        {pct === null ? '—' : `${Math.round(pct)} %`}
      </Text>
      <Text style={styles.miniSuffix}>{suffix}</Text>
    </View>
  );
}

/**
 * Colour by magnitude, not by good or bad. A wide day is not a buy signal and
 * a narrow one is not a warning — the palette says "unusual", never "act".
 */
function adrColour(pct: number): string {
  if (pct < 70) return neon.cyan; // quieter than usual
  if (pct <= 130) return neon.green; // ordinary
  return neon.violet; // notably wide
}

function biasLabel(read: BiasRead): string {
  switch (read) {
    case 'continuation_haussiere':
      return 'Continuation haussière';
    case 'continuation_baissiere':
      return 'Continuation baissière';
    case 'reversal_depuis_le_haut':
      return 'Reversal depuis le haut';
    case 'reversal_depuis_le_bas':
      return 'Reversal depuis le bas';
    case 'zero_cinq_traverse':
      return '0,5 traversé dans les deux sens';
    default:
      return 'Indéterminé';
  }
}

function biasColour(read: BiasRead): string {
  switch (read) {
    case 'continuation_haussiere':
    case 'reversal_depuis_le_bas':
      return neon.green;
    case 'continuation_baissiere':
    case 'reversal_depuis_le_haut':
      return neon.red;
    case 'zero_cinq_traverse':
      return neon.cyan;
    default:
      return neon.muted;
  }
}

/** One rung: label, price, and the gap to the level above it. */
function LadderLine({
  row,
  decimals,
  previous,
  last,
}: {
  row: LadderRow;
  decimals: number;
  previous: LadderRow | null;
  last: boolean;
}) {
  const gap = previous ? previous.price - row.price : null;

  return (
    <View style={[styles.ladderRow, last && styles.lastRow]}>
      <View style={[styles.tick, { backgroundColor: KIND_COLOR[row.kind] }]} />
      <View style={{ flex: 1 }}>
        <Text style={styles.ladderLabel} numberOfLines={1}>
          {row.label}
        </Text>
        {row.swept ? (
          <Text style={styles.ladderSwept}>pris à {parisTime(row.sweptAt)}</Text>
        ) : gap !== null && gap > 0 ? (
          <Text style={styles.ladderGap}>
            {gap.toLocaleString('fr-FR', { maximumFractionDigits: 1 })} pts plus bas
          </Text>
        ) : null}
      </View>
      <Text style={[styles.ladderPrice, row.swept && styles.ladderPriceSwept]}>
        {formatPrice(row.price, decimals)}
      </Text>
    </View>
  );
}

function SessionCard({ window, decimals }: { window: LevelWindow; decimals: number }) {
  return (
    <View style={styles.sessionCard}>
      <Text style={styles.sessionName} numberOfLines={1}>
        {window.label}
      </Text>
      <Text style={styles.sessionRange}>
        {formatPoints(window.range ?? 0, decimals)} pts
      </Text>
      <Text style={styles.sessionMeta}>
        {formatPrice(window.low ?? 0, decimals)} – {formatPrice(window.high ?? 0, decimals)}
      </Text>
      <Text style={styles.sessionMeta}>
        {parisTime(window.start)} → {parisTime(window.end)}
      </Text>
    </View>
  );
}

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <View style={styles.card}>
      <Text style={styles.emptyTitle}>{title}</Text>
      <Text style={styles.emptyBody}>{body}</Text>
    </View>
  );
}

function glowText(color: string) {
  return { textShadowColor: color, textShadowRadius: 10, textShadowOffset: { width: 0, height: 0 } };
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: neon.bg },
  content: { paddingBottom: 48 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: neon.bg },
  error: { color: neon.red, paddingHorizontal: 18, paddingTop: 8 },

  symbolBar: { paddingHorizontal: 16, paddingTop: 12, gap: 8 },
  symbolPill: {
    paddingHorizontal: 16,
    paddingVertical: 8,
    borderRadius: 12,
    backgroundColor: neon.panel,
    borderWidth: 1,
    borderColor: neon.border,
  },
  symbolPillOn: { backgroundColor: 'rgba(34,211,238,0.12)', borderColor: 'rgba(34,211,238,0.45)' },
  symbolText: { color: neon.muted, fontSize: 13, fontWeight: '600' },
  symbolTextOn: { color: neon.cyan },

  dateBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginHorizontal: 16,
    marginTop: 14,
  },
  arrow: { paddingHorizontal: 14, paddingVertical: 2 },
  arrowText: { color: neon.cyan, fontSize: 26, lineHeight: 28 },
  arrowOff: { color: neon.border },
  dateText: { color: neon.text, fontSize: 16, fontWeight: '600', textAlign: 'center' },
  dateHint: { color: neon.muted, fontSize: 10, textAlign: 'center', marginTop: 1 },

  card: {
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 18,
    padding: 16,
    marginHorizontal: 16,
    marginTop: 14,
  },
  lbl: { color: neon.muted, fontSize: 11, letterSpacing: 0.4, textTransform: 'uppercase' },
  hero: { color: neon.text, fontSize: 30, fontWeight: '700', marginTop: 2 },
  heroRow: { flexDirection: 'row', justifyContent: 'space-between', marginTop: 14 },
  heroSide: { fontSize: 19, fontWeight: '700', marginTop: 2 },
  heroTime: { color: neon.muted, fontSize: 11, marginTop: 1 },

  sectionLbl: {
    color: neon.muted,
    fontSize: 11,
    letterSpacing: 0.5,
    textTransform: 'uppercase',
    marginHorizontal: 18,
    marginTop: 20,
    marginBottom: 6,
  },

  listCard: {
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 14,
    paddingHorizontal: 14,
    marginHorizontal: 16,
  },
  ladderRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: 10,
    borderBottomColor: neon.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  lastRow: { borderBottomWidth: 0 },
  tick: { width: 3, height: 26, borderRadius: 2, marginRight: 11 },
  ladderLabel: { color: neon.text, fontSize: 14, fontWeight: '500' },
  ladderGap: { color: neon.muted, fontSize: 11, marginTop: 1 },
  ladderSwept: { color: neon.violet, fontSize: 11, marginTop: 1 },
  ladderPrice: { color: neon.text, fontSize: 15, fontWeight: '700', fontVariant: ['tabular-nums'] },
  ladderPriceSwept: { color: neon.muted, textDecorationLine: 'line-through' },

  moreBtn: { alignSelf: 'center', marginTop: 10, paddingVertical: 6, paddingHorizontal: 14 },
  moreText: { color: neon.cyan, fontSize: 13, fontWeight: '600' },

  sweepRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: 11,
    borderBottomColor: neon.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  sweepLabel: { color: neon.text, fontSize: 14, fontWeight: '500' },
  sweepMeta: { color: neon.muted, fontSize: 11, marginTop: 2 },
  sweepExc: { color: neon.green, fontSize: 15, fontWeight: '700' },

  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: 12, marginHorizontal: 16 },
  sessionCard: {
    width: '47%',
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 14,
    padding: 13,
  },
  sessionName: { color: neon.text, fontSize: 13, fontWeight: '600' },
  sessionRange: { color: neon.cyan, fontSize: 17, fontWeight: '700', marginTop: 4 },
  sessionMeta: { color: neon.muted, fontSize: 11, marginTop: 3 },

  biasMid: { color: neon.text, fontSize: 30, fontWeight: '800', marginTop: 2 },
  biasRead: { fontSize: 16, fontWeight: '700', marginTop: 4 },
  biasOpenRow: { flexDirection: 'row', alignItems: 'center', marginTop: 10 },
  biasOpenText: { color: neon.text, fontSize: 13, flex: 1 },
  biasOpenHint: { color: neon.muted, fontSize: 11 },
  biasFootnote: { color: neon.muted, fontSize: 10, lineHeight: 14, marginTop: 8 },

  flagGrid: { flexDirection: 'row', flexWrap: 'wrap' },
  flag: { width: '50%', flexDirection: 'row', alignItems: 'center', paddingVertical: 4 },
  flagMark: { fontSize: 14, fontWeight: '700', width: 18 },
  flagLabel: { color: neon.muted, fontSize: 12, flexShrink: 1 },

  weekRow: { flexDirection: 'row', marginTop: 10 },
  weekDay: { color: neon.muted, fontSize: 11, textTransform: 'capitalize' },
  weekValue: { fontSize: 18, fontWeight: '700', marginTop: 2 },

  adrRow: { flexDirection: 'row', alignItems: 'baseline', gap: 8, marginTop: 2 },
  adrValue: { fontSize: 34, fontWeight: '800' },
  adrCaption: { color: neon.muted, fontSize: 12, flexShrink: 1 },
  adrDetail: { color: neon.muted, fontSize: 11, marginTop: 8, lineHeight: 16 },

  barTrack: {
    height: 8,
    borderRadius: 4,
    backgroundColor: neon.track,
    marginTop: 12,
    overflow: 'hidden',
    justifyContent: 'center',
  },
  barFill: { position: 'absolute', left: 0, top: 0, bottom: 0, borderRadius: 4 },
  barTick: {
    position: 'absolute',
    left: '50%',
    width: 2,
    top: 0,
    bottom: 0,
    backgroundColor: neon.bg,
    opacity: 0.9,
  },

  contextDivider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: neon.border,
    marginVertical: 14,
  },
  contextRow: { flexDirection: 'row', alignItems: 'flex-start' },
  contextSplit: { gap: 12, marginLeft: 12 },
  trendValue: { fontSize: 19, fontWeight: '700', marginTop: 2 },
  contextMeta: { color: neon.muted, fontSize: 11, marginTop: 3, lineHeight: 15 },
  miniValue: { fontSize: 16, fontWeight: '700', marginTop: 2 },
  miniSuffix: { color: neon.muted, fontSize: 10 },

  observationRow: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingVertical: 10,
    borderBottomColor: neon.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  bullet: {
    width: 5,
    height: 5,
    borderRadius: 3,
    backgroundColor: neon.cyan,
    marginTop: 7,
    marginRight: 10,
  },
  observationText: { color: neon.text, fontSize: 13, lineHeight: 19, flex: 1 },

  modeBar: {
    flexDirection: 'row',
    gap: 6,
    backgroundColor: neon.panel,
    borderColor: neon.border,
    borderWidth: 1,
    borderRadius: 14,
    padding: 5,
    marginHorizontal: 16,
    marginTop: 12,
  },
  modeBtn: { flex: 1, alignItems: 'center', paddingVertical: 9, borderRadius: 10 },
  modeBtnOn: { backgroundColor: 'rgba(34,211,238,0.14)' },
  modeText: { color: neon.muted, fontSize: 13, fontWeight: '600' },
  modeTextOn: { color: neon.cyan },

  centeredBlock: { paddingVertical: 40, alignItems: 'center' },
  reviewRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: 12,
    borderBottomColor: neon.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  reviewDate: { color: neon.text, fontSize: 14, fontWeight: '600', textTransform: 'capitalize' },
  reviewBias: { fontSize: 12, fontWeight: '600', marginTop: 2 },
  reviewSessions: { color: neon.muted, fontSize: 10, marginTop: 1 },
  reviewTrades: { color: neon.muted, fontSize: 12 },
  reviewPnl: { fontSize: 15, fontWeight: '700', marginTop: 2 },

  emptyTitle: { color: neon.text, fontSize: 15, fontWeight: '600' },
  emptyBody: { color: neon.muted, fontSize: 13, lineHeight: 19, marginTop: 6 },
  emptyLine: { color: neon.muted, fontSize: 13, paddingVertical: 14 },
  footnote: {
    color: neon.muted,
    fontSize: 11,
    lineHeight: 16,
    marginHorizontal: 18,
    marginTop: 20,
  },
});
