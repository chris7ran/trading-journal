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
