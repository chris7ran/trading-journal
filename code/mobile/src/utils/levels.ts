// Pure helpers for the "Niveaux du jour" screen.
//
// The backend does the market maths; this file only turns its payload into
// something readable on a phone: a price ladder sorted the way you would draw
// it on a chart, prices formatted per instrument, and timestamps shown in
// Paris time.

import type { DailyLevels, LevelWindow, Sweep } from '../api/types';
import { neon } from '../theme-neon';

/** One line of the price ladder. */
export interface LadderRow {
  key: string;
  label: string;
  price: number;
  /** Drives the colour and the grouping. */
  kind: 'day' | 'previous' | 'orb' | 'session';
  side: 'high' | 'low';
  /** True when a sweep fired on (or extremely close to) this level. */
  swept: boolean;
  /** Shown only for swept levels: when it was taken, Paris time. */
  sweptAt: string | null;
  /** Levels shown before the user expands the ladder. */
  essential: boolean;
}

/**
 * Colour per ladder kind. Lives here rather than in either consumer so that a
 * level is the same colour in the list and on the chart — reading one against
 * the other is the whole point of showing both.
 */
export const KIND_COLOR: Record<LadderRow['kind'], string> = {
  day: neon.cyan,
  previous: neon.violet,
  orb: neon.green,
  session: neon.muted,
};

/**
 * Compact name for a level on a chart, where there is room for two words.
 *
 * `label` spells the side out in prose ("PDH — haut de la veille"); the head of
 * it is enough, except for the windows whose name says nothing about which
 * edge this is.
 */
export function chartTag(row: LadderRow): string {
  const head = row.label.split(' — ')[0];
  const carriesSide = /haut|bas|pdh|pdl|0,5/i.test(head);
  return carriesSide ? head : `${head} ${row.side === 'high' ? 'H' : 'B'}`;
}

// --- Time -------------------------------------------------------------------

/**
 * UTC ISO string -> `HH:MM` in Paris time.
 *
 * Deliberately hand-rolled rather than `Intl.DateTimeFormat` with a `timeZone`:
 * Hermes' Intl support varies by platform and RN version, and a silently wrong
 * hour here would be worse than an obvious crash. The EU rule is uniform and
 * legislated — CEST from the last Sunday of March 01:00 UTC to the last Sunday
 * of October 01:00 UTC — so a single zone is safe to compute directly.
 *
 * Note this is display only. Every window boundary was already resolved
 * server-side with a real timezone database.
 */
export function parisTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '—';

  const shifted = new Date(t + parisOffsetMinutes(t) * 60_000);
  const hh = String(shifted.getUTCHours()).padStart(2, '0');
  const mm = String(shifted.getUTCMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

function parisOffsetMinutes(ts: number): number {
  const year = new Date(ts).getUTCFullYear();
  const summerStart = lastSundayUtc(year, 2, 1); // March, 01:00 UTC
  const summerEnd = lastSundayUtc(year, 9, 1); // October, 01:00 UTC
  return ts >= summerStart && ts < summerEnd ? 120 : 60;
}

/** Epoch ms of the last Sunday of `month` (0-based) at `hourUtc`. */
function lastSundayUtc(year: number, month: number, hourUtc: number): number {
  // Day 0 of the following month is the last day of this one.
  const last = new Date(Date.UTC(year, month + 1, 0));
  const sunday = last.getUTCDate() - last.getUTCDay();
  return Date.UTC(year, month, sunday, hourUtc);
}

// --- Dates ------------------------------------------------------------------

/** Today as `YYYY-MM-DD`, in Paris terms. */
export function todayParis(): string {
  const now = Date.now();
  return isoDate(new Date(now + parisOffsetMinutes(now) * 60_000));
}

/** The Paris calendar day an ISO UTC instant falls on. */
export function parisDate(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  return isoDate(new Date(t + parisOffsetMinutes(t) * 60_000));
}

/** Shift a `YYYY-MM-DD` string by whole days without touching timezones. */
export function shiftDate(date: string, days: number): string {
  const [y, m, d] = date.split('-').map(Number);
  return isoDate(new Date(Date.UTC(y, m - 1, d + days)));
}

function isoDate(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/** `2026-08-28` -> `ven. 28 août`. */
export function humanDate(date: string): string {
  const [y, m, d] = date.split('-').map(Number);
  const dt = new Date(Date.UTC(y, m - 1, d));
  const days = ['dim.', 'lun.', 'mar.', 'mer.', 'jeu.', 'ven.', 'sam.'];
  const months = [
    'janv.', 'févr.', 'mars', 'avr.', 'mai', 'juin',
    'juil.', 'août', 'sept.', 'oct.', 'nov.', 'déc.',
  ];
  return `${days[dt.getUTCDay()]} ${dt.getUTCDate()} ${months[dt.getUTCMonth()]}`;
}

// --- Prices -----------------------------------------------------------------

/**
 * Decimal places to show. Indices and metals quote in hundredths; FX pairs sit
 * around 1.xxxx and need four. Inferring from magnitude beats hard-coding a
 * per-symbol table that would rot the first time a broker renames something.
 */
export function priceDecimals(reference: number): number {
  return Math.abs(reference) >= 100 ? 2 : 4;
}

export function formatPrice(value: number, decimals: number): string {
  return value.toLocaleString('fr-FR', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

/** Signed distance in points, e.g. `+126,2` — no currency, these are index points. */
export function formatPoints(value: number, decimals: number): string {
  const sign = value > 0 ? '+' : value < 0 ? '−' : '';
  return `${sign}${Math.abs(value).toLocaleString('fr-FR', {
    minimumFractionDigits: Math.min(decimals, 1),
    maximumFractionDigits: Math.min(decimals, 1),
  })}`;
}

// --- Ladder -----------------------------------------------------------------

/** Sessions worth putting on the ladder by default, in the trader's language. */
const SESSION_SHORT: Record<string, string> = {
  asia_range: 'Asie',
  tokyo: 'Tokyo',
  london: 'Londres',
  newyork: 'New York',
};

/** Only these appear before the user taps "tout afficher". */
const ESSENTIAL_SESSIONS = new Set(['asia_range']);

/**
 * Build the price ladder: every meaningful level, highest first, the way you
 * would mark them on a chart before the open.
 *
 * A level is flagged as swept when a sweep in the payload fired on the same
 * price. Comparison is by proximity rather than equality because the same
 * number arrives through two different float paths.
 */
export function buildLadder(levels: DailyLevels): LadderRow[] {
  const rows: LadderRow[] = [];
  const tolerance = ladderTolerance(levels);

  const push = (
    key: string,
    label: string,
    price: number | undefined,
    kind: LadderRow['kind'],
    side: LadderRow['side'],
    essential: boolean,
  ) => {
    if (price === undefined || !Number.isFinite(price)) return;
    const hit = matchSweep(levels.sweeps, price, tolerance);
    rows.push({
      key,
      label,
      price,
      kind,
      side,
      essential,
      swept: hit?.swept ?? false,
      sweptAt: hit?.swept ? hit.at : null,
    });
  };

  if (levels.day) {
    push('day_high', 'Haut du jour', levels.day.high, 'day', 'high', true);
    push('day_low', 'Bas du jour', levels.day.low, 'day', 'low', true);
  }

  if (levels.previous_day) {
    push('pdh', 'PDH — haut de la veille', levels.previous_day.high, 'previous', 'high', true);
    push('pdl', 'PDL — bas de la veille', levels.previous_day.low, 'previous', 'low', true);
    // The midpoint of yesterday's range: the level the daily bias is read
    // against, so it belongs on the ladder next to the extremes it sits between.
    push(
      'previous_mid',
      '0,5 de la veille',
      (levels.previous_day.high + levels.previous_day.low) / 2,
      'previous',
      'high',
      true,
    );
  }

  for (const orb of levels.opening_ranges) {
    const name = orb.key === 'orb_london' ? 'ORB Londres' : orb.key === 'orb_newyork' ? 'ORB New York' : orb.label;
    push(`${orb.key}_high`, `${name} — haut`, orb.high, 'orb', 'high', true);
    push(`${orb.key}_low`, `${name} — bas`, orb.low, 'orb', 'low', true);
  }

  for (const session of levels.sessions) {
    const name = SESSION_SHORT[session.key] ?? session.label;
    const essential = ESSENTIAL_SESSIONS.has(session.key);
    push(`${session.key}_high`, `${name} — haut`, session.high, 'session', 'high', essential);
    push(`${session.key}_low`, `${name} — bas`, session.low, 'session', 'low', essential);
  }

  // Highest first: the ladder reads like a chart.
  rows.sort((a, b) => b.price - a.price);
  return rows;
}

/**
 * Tolerance for "is this the same level?", scaled to the instrument.
 *
 * One ten-thousandth of the day's range: tight enough that two genuinely
 * different levels never merge, loose enough to absorb float representation
 * differences on a 26,000-point index.
 */
function ladderTolerance(levels: DailyLevels): number {
  const range = levels.day?.range ?? levels.previous_day?.range ?? 0;
  return Math.max(range / 10_000, 1e-6);
}

function matchSweep(sweeps: Sweep[], price: number, tolerance: number): Sweep | undefined {
  return sweeps.find((s) => Math.abs(s.level - price) <= tolerance);
}

/** Windows that actually have data, for the compact session recap. */
export function withData(windows: LevelWindow[]): LevelWindow[] {
  return windows.filter((w) => w.high !== undefined && w.low !== undefined);
}

/** True when the payload holds nothing worth rendering. */
export function isEmpty(levels: DailyLevels | null): boolean {
  return !levels || (!levels.day && withData(levels.sessions).length === 0);
}
