// Dashboard / Stats filters: time period + instrument.

export type Period = 'all' | '30d' | '90d' | 'ytd';

export interface Filters {
  period: Period;
  symbol: string | null; // null = all instruments
  accountId: string | null; // null = all accounts (total)
}

export const DEFAULT_FILTERS: Filters = { period: 'all', symbol: null, accountId: null };

export const PERIOD_LABELS: Record<Period, string> = {
  all: 'Tout',
  '30d': '30j',
  '90d': '90j',
  ytd: 'YTD',
};

function isoDaysAgo(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return d.toISOString().slice(0, 19); // YYYY-MM-DDTHH:MM:SS
}

/** Translate filters into API query params (`from`, `to`, `symbol`). */
export function filtersToQuery(f: Filters): {
  from?: string;
  to?: string;
  symbol?: string;
  account_id?: string;
} {
  const out: { from?: string; to?: string; symbol?: string; account_id?: string } = {};
  if (f.period === '30d') out.from = isoDaysAgo(30);
  else if (f.period === '90d') out.from = isoDaysAgo(90);
  else if (f.period === 'ytd') out.from = `${new Date().getFullYear()}-01-01T00:00:00`;
  if (f.symbol) out.symbol = f.symbol;
  if (f.accountId) out.account_id = f.accountId;
  return out;
}
