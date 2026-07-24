// Asset selector for the Macro screens (calendar + news).
// Calendar filters by currency; news filters by keyword match on the title.

import type { EcoEvent, EconIndicator, NewsItem } from '../api/types';

export type Asset = 'all' | 'USD' | 'EUR' | 'JPY' | 'US30' | 'DAX40';

export const ASSETS: Asset[] = ['all', 'USD', 'EUR', 'JPY', 'US30', 'DAX40'];

export const ASSET_LABELS: Record<Asset, string> = {
  all: 'Tout',
  USD: 'USD',
  EUR: 'EUR',
  JPY: 'JPY',
  US30: 'US30',
  DAX40: 'DAX40',
};

const CURRENCIES: Record<Asset, string[]> = {
  all: [],
  USD: ['USD'],
  EUR: ['EUR'],
  JPY: ['JPY'],
  US30: ['USD'],
  DAX40: ['EUR'],
};

const KEYWORDS: Record<Asset, string[]> = {
  all: [],
  USD: ['dollar', 'usd', 'fed', 'fomc', 'powell', 'treasury', 'jobless', 'nfp', 'u.s.', 'united states', 'cpi'],
  EUR: ['euro', 'eur', 'ecb', 'lagarde', 'eurozone', 'bund'],
  JPY: ['yen', 'jpy', 'boj', 'ueda', 'japan', 'japanese', 'tokyo', 'nikkei'],
  US30: ['dow', 'djia', 'wall street', 'blue-chip', 'us30', 'nasdaq', 's&p', 'sp500'],
  DAX40: ['dax', 'ger40', 'germany', 'german', 'frankfurt', 'bundesbank'],
};

/** Keep an economic event if it matches the selected asset's currency. */
export function matchEvent(e: EcoEvent, asset: Asset): boolean {
  if (asset === 'all') return true;
  return CURRENCIES[asset].includes(e.currency);
}

// Region (or market group) -> currency, for filtering macro indicator cards.
const INDICATOR_CCY: Record<string, string> = {
  'États-Unis': 'USD',
  'Zone euro': 'EUR',
  Allemagne: 'EUR',
  'Royaume-Uni': 'GBP',
  Japon: 'JPY',
  Or: 'USD', // USD-denominated / US-rate driven
  Pétrole: 'USD',
  'Taux US': 'USD',
};

/** Keep a macro indicator card if it matches the selected asset. */
export function matchIndicator(ind: EconIndicator, asset: Asset): boolean {
  if (asset === 'all') return true;
  const c = INDICATOR_CCY[ind.region];
  if (!c) return true; // unknown region -> keep visible
  return CURRENCIES[asset].includes(c);
}

/** Keep a news item if its title contains one of the asset's keywords. */
export function matchNews(n: NewsItem, asset: Asset): boolean {
  if (asset === 'all') return true;
  const title = n.title.toLowerCase();
  return KEYWORDS[asset].some((k) => title.includes(k));
}
