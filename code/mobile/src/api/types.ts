// API types mirroring the Rust backend's JSON shapes.

export interface Trade {
  id: string;
  account_id: string;
  symbol: string;
  direction: string | null;
  open_time: string | null;
  close_time: string | null;
  open_price: number | null;
  close_price: number | null;
  stop_loss: number | null;
  take_profit: number | null;
  lot_size: number | null;
  pnl: number | null;
  pnl_pct: number | null;
  commission: number | null;
  swap: number | null;
  setup_tag: string | null;
  emotion_tag: string | null;
  notes: string | null;
  screenshot_url: string | null;
  mt5_ticket: string | null;
  followed_plan: boolean | null;
  respected_sl: boolean | null;
  pattern_valid: boolean | null;
  thesis_worked: boolean | null;
  good_exit: boolean | null;
  created_at: string;
}

/** Post-trade review flags. */
export interface TradeReview {
  followed_plan?: boolean | null;
  respected_sl?: boolean | null;
  pattern_valid?: boolean | null;
  thesis_worked?: boolean | null;
  good_exit?: boolean | null;
}

export interface Setup {
  id: string;
  name: string;
  rules: string | null;
  description: string | null;
  target_entry: string | null;
  target_exit: string | null;
  stop_loss: string | null;
  created_at: string;
}

export interface NewSetup {
  name: string;
  rules?: string | null;
  description?: string | null;
  target_entry?: string | null;
  target_exit?: string | null;
  stop_loss?: string | null;
}

/** Payload for PUT /setups/:id (partial — omitted fields keep their value). */
export interface UpdateSetup {
  name?: string | null;
  rules?: string | null;
  description?: string | null;
  target_entry?: string | null;
  target_exit?: string | null;
  stop_loss?: string | null;
}

export interface SymbolStat {
  symbol: string;
  trades: number;
  wins: number;
  pnl: number;
}

export interface TradeStats {
  total_trades: number;
  wins: number;
  losses: number;
  win_rate: number; // 0..1
  total_pnl: number;
  avg_win: number;
  avg_loss: number;
  profit_factor: number | null;
  gross_profit: number;
  gross_loss: number;
  by_symbol: SymbolStat[];
}

export interface LoginResponse {
  token: string;
  token_type: string;
  expires_in_hours: number;
}

/** Response of POST /trades/import/csv. */
export interface ImportSummary {
  imported: number;
  skipped_duplicates: number;
  failed: number;
  warnings: string[];
  /** Account the trades landed in (resolved from the report or chosen). */
  account_id?: string;
  account_name?: string;
}

/** Payload for POST /trades (manual entry). All fields optional except symbol. */
export interface NewTrade extends TradeReview {
  account_id?: string;
  symbol: string;
  direction?: string | null;
  open_time?: string | null;
  close_time?: string | null;
  open_price?: number | null;
  close_price?: number | null;
  stop_loss?: number | null;
  take_profit?: number | null;
  lot_size?: number | null;
  pnl?: number | null;
  setup_tag?: string | null;
  emotion_tag?: string | null;
  notes?: string | null;
  screenshot_url?: string | null;
  mt5_ticket?: string | null;
}

/** Payload for PUT /trades/:id (partial update). */
export type UpdateTrade = Partial<Omit<NewTrade, 'account_id' | 'mt5_ticket'>>;

// --- Accounts & prop firm rules ---------------------------------------------

export interface Account {
  id: string;
  name: string;
  broker: string;
  balance: number | null;
  currency: string;
  is_funded: boolean;
  created_at: string;
}

export interface PropRule {
  id: string;
  account_id: string;
  daily_drawdown_max: number | null;   // fraction, e.g. 0.05 = 5%
  global_drawdown_max: number | null;
  profit_target: number | null;        // fraction, e.g. 0.10 = 10%
  min_trading_days: number | null;
  consistency_rule_pct: number;
  lot_size_max: number | null;
}

export interface UpsertPropRule {
  daily_drawdown_max?: number | null;
  global_drawdown_max?: number | null;
  profit_target?: number | null;
  min_trading_days?: number | null;
  consistency_rule_pct?: number | null;
  lot_size_max?: number | null;
}

export interface UpdateAccount {
  name?: string;
  balance?: number | null;
  currency?: string;
  is_funded?: boolean;
}

export interface NewAccount {
  name: string;
  broker?: string;
  balance?: number | null;
  currency?: string;
  is_funded?: boolean;
}

// --- Macro terminal ---------------------------------------------------------

export interface EcoEvent {
  title: string;
  currency: string;
  impact: 'red' | 'orange' | 'yellow';
  date: string; // ISO-8601 with offset
  forecast: string | null;
  previous: string | null;
}

export interface NewsItem {
  title: string;
  url: string;
  source: string;
  published_at: string | null;
  sentiment: 'bullish' | 'bearish' | 'neutral';
}

export interface EconIndicator {
  label: string;
  region: string;
  unit: string;
  year: string; // year (macro) or date (market)
  value: number;
  previous: number | null;
  history: number[]; // oldest -> newest
  category: 'macro' | 'market' | 'macro_monthly';
}

/** Weekly CFTC Commitments of Traders positioning (leveraged funds). */
export interface CotEntry {
  marche: string; // "S&P 500" | "EUR" ...
  net: number; // long - short
  chg_hebdo: number; // week-over-week change in net
  date: string; // YYYY-MM-DD
}

/** Optional filters for GET /trades. */
export interface TradeFilters {
  account_id?: string;
  symbol?: string;
  direction?: string;
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
}

// --- Daily levels (GET /levels) ---------------------------------------------

/** Aggregated OHLC over a window. `range` is high - low, in price units. */
export interface Ohlc {
  open: number;
  high: number;
  low: number;
  close: number;
  range: number;
  bars: number;
  high_at: string; // ISO UTC
  low_at: string; // ISO UTC
}

/**
 * A named session or opening range. The OHLC fields are flattened by the API
 * and are **absent** when the market was closed or no candle was received —
 * hence `Partial`.
 */
export interface LevelWindow extends Partial<Ohlc> {
  key: string;
  label: string;
  timezone: string;
  start: string; // ISO UTC
  end: string; // ISO UTC
}

export interface PreviousDay extends Ohlc {
  date: string; // YYYY-MM-DD
}

/** "Did price take out this level during that window, and when?" */
export interface Sweep {
  key: string;
  label: string;
  level: number;
  swept: boolean;
  at: string | null; // ISO UTC, first breach
  excursion: number; // how far beyond the level, in price units
}

export interface DailyLevels {
  symbol: string;
  date: string; // YYYY-MM-DD, Paris calendar day
  timezone: string;
  day: Ohlc | null;
  previous_day: PreviousDay | null;
  sessions: LevelWindow[];
  opening_ranges: LevelWindow[];
  sweeps: Sweep[];
}

/** One entry of GET /levels/symbols — what the MT5 feed has delivered. */
export interface LevelSymbol {
  symbol: string;
  count: number;
  first: string; // ISO UTC
  last: string; // ISO UTC
}

// --- Morning brief (GET /brief) ---------------------------------------------

/**
 * Range statistics for the day, all expressed against the instrument's own
 * recent behaviour so the same number means the same thing on GER40 and on
 * XAUUSD. `null` when there is too little history to be honest about.
 */
export interface RangeStats {
  /** Completed sessions behind the averages. */
  sessions: number;
  /** Average daily range over the lookback, in price units. */
  adr: number | null;
  adr_median: number | null;
  day_range: number | null;
  /** Today's range as a percentage of the ADR. 100 = an average day. */
  day_pct_of_adr: number | null;
  previous_day_range: number | null;
  previous_day_pct_of_adr: number | null;
  asia_range: number | null;
  asia_median: number | null;
  asia_pct_of_median: number | null;
}

export type TrendDirection = 'up' | 'down' | 'range' | 'unknown';

export interface TrendRead {
  direction: TrendDirection;
  sessions: number;
  sma: number | null;
  /**
   * Last close of the lookback window — the session *before* the requested
   * date, not today's close. The trend is read on completed sessions only.
   */
  window_last_close: number | null;
  close_vs_sma_pct_of_adr: number | null;
  net_move: number | null;
  /** Net move measured in average days. Above 1 is what makes it a trend. */
  net_move_in_adr: number | null;
  higher_highs_5: number;
  lower_lows_5: number;
}

export interface LevelDistance {
  key: string;
  label: string;
  price: number;
  /** Signed: positive when the level sits above the reference price. */
  distance: number;
  pct_of_adr: number | null;
  above: boolean;
  swept: boolean;
}

/** A measurement stated in French. Never a recommendation. */
export interface Observation {
  key: string;
  text: string;
}

export interface DailyBrief {
  symbol: string;
  date: string;
  /** Latest close available — what the distances are measured from. */
  reference_price: number | null;
  stats: RangeStats;
  trend: TrendRead;
  distances: LevelDistance[];
  observations: Observation[];
  /** Levels travel with the brief so one screen needs one request. */
  levels: DailyLevels;
}
