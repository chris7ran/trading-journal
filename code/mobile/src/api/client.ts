// Tiny typed fetch client for the Trading Journal backend.
//
// `createApi(baseUrl, token?)` returns bound methods. The base URL points at
// your self-hosted backend (LAN IP or Tailscale hostname, e.g.
// `http://proxmox-journal.tailXXXX.ts.net:8080`).

import type {
  Account,
  EcoEvent,
  EconIndicator,
  ImportSummary,
  LoginResponse,
  NewAccount,
  NewsItem,
  NewSetup,
  NewTrade,
  PropRule,
  Setup,
  Trade,
  TradeFilters,
  TradeStats,
  UpdateAccount,
  UpdateSetup,
  UpdateTrade,
  UpsertPropRule,
} from './types';

/** Error carrying the HTTP status, so screens can react (e.g. 401 -> logout). */
export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

function normalizeBaseUrl(raw: string): string {
  let url = raw.trim().replace(/\/+$/, ''); // strip trailing slashes
  if (!/^https?:\/\//i.test(url)) {
    url = `http://${url}`; // default to http for LAN/Tailscale
  }
  return url;
}

export function createApi(baseUrl: string, token?: string | null) {
  const base = normalizeBaseUrl(baseUrl);

  // Shared response handling: map !ok to ApiError, parse JSON otherwise.
  async function parseResponse<T>(res: Response): Promise<T> {
    if (!res.ok) {
      let message = `Erreur ${res.status}`;
      try {
        const body = await res.json();
        if (body?.error) message = body.error;
      } catch {
        // non-JSON body, keep default message
      }
      throw new ApiError(res.status, message);
    }
    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
  }

  async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...(options.headers as Record<string, string> | undefined),
    };
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }

    let res: Response;
    try {
      res = await fetch(`${base}${path}`, { ...options, headers });
    } catch (e) {
      throw new ApiError(0, `Réseau injoignable (${base}). Vérifie l'URL et le VPN.`);
    }
    return parseResponse<T>(res);
  }

  function buildQuery(filters?: object): string {
    if (!filters) return '';
    const params = new URLSearchParams();
    Object.entries(filters).forEach(([k, v]) => {
      if (v !== undefined && v !== null && v !== '') params.append(k, String(v));
    });
    const qs = params.toString();
    return qs ? `?${qs}` : '';
  }

  return {
    baseUrl: base,

    /** Health check (unauthenticated). */
    health: () => request<{ status: string }>('/health'),

    /** Exchange the password for a JWT. */
    login: (password: string) =>
      request<LoginResponse>('/auth/login', {
        method: 'POST',
        body: JSON.stringify({ password }),
      }),

    /** List trades (newest first), with optional filters. */
    listTrades: (filters?: TradeFilters) =>
      request<Trade[]>(`/trades${buildQuery(filters)}`),

    /** Create a trade (manual entry). */
    createTrade: (body: NewTrade) =>
      request<Trade>('/trades', { method: 'POST', body: JSON.stringify(body) }),

    /** Partially update a trade. */
    updateTrade: (id: string, patch: UpdateTrade) =>
      request<Trade>(`/trades/${encodeURIComponent(id)}`, {
        method: 'PUT',
        body: JSON.stringify(patch),
      }),

    /** Delete a trade. */
    deleteTrade: (id: string) =>
      request<void>(`/trades/${encodeURIComponent(id)}`, { method: 'DELETE' }),

    /** Aggregated analytics. */
    getStats: (filters?: Partial<TradeFilters>) =>
      request<TradeStats>(`/trades/stats${buildQuery(filters)}`),

    /** List trading accounts. */
    listAccounts: () => request<Account[]>('/accounts'),

    /** Create a trading account. */
    createAccount: (body: NewAccount) =>
      request<Account>('/accounts', { method: 'POST', body: JSON.stringify(body) }),

    /** Partially update an account (e.g. starting balance). */
    updateAccount: (id: string, patch: UpdateAccount) =>
      request<Account>(`/accounts/${encodeURIComponent(id)}`, {
        method: 'PUT',
        body: JSON.stringify(patch),
      }),

    /** Delete an account and, in cascade, its trades and prop rules. */
    deleteAccount: (id: string) =>
      request<void>(`/accounts/${encodeURIComponent(id)}`, { method: 'DELETE' }),

    /** Get prop firm rules for an account (throws 404 if unset). */
    getRules: (accountId: string) =>
      request<PropRule>(`/accounts/${encodeURIComponent(accountId)}/rules`),

    /** Insert-or-update prop firm rules. */
    upsertRules: (accountId: string, rules: UpsertPropRule) =>
      request<PropRule>(`/accounts/${encodeURIComponent(accountId)}/rules`, {
        method: 'PUT',
        body: JSON.stringify(rules),
      }),

    /** Economic calendar (high/medium impact) for the current week. */
    getCalendar: () => request<EcoEvent[]>('/macro/calendar'),

    /** Aggregated market news headlines. */
    getNews: () => request<NewsItem[]>('/macro/news'),

    /** Key macro economic indicators (World Bank). */
    getEconomy: () => request<EconIndicator[]>('/macro/economy'),

    /** List trading setups / patterns. */
    listSetups: () => request<Setup[]>('/setups'),

    /** Create a setup. */
    createSetup: (body: NewSetup) =>
      request<Setup>('/setups', { method: 'POST', body: JSON.stringify(body) }),

    /** Update a setup (partial). */
    updateSetup: (id: string, body: UpdateSetup) =>
      request<Setup>(`/setups/${id}`, { method: 'PUT', body: JSON.stringify(body) }),

    /**
     * Import an MT5 export (.xlsx or .csv) by sending its raw bytes.
     * The backend auto-detects the format. `account_id` is optional.
     */
    async importFile(blob: Blob, accountId?: string): Promise<ImportSummary> {
      const headers: Record<string, string> = {
        'Content-Type': 'application/octet-stream',
      };
      if (token) headers.Authorization = `Bearer ${token}`;
      const qs = accountId ? `?account_id=${encodeURIComponent(accountId)}` : '';

      let res: Response;
      try {
        res = await fetch(`${base}/trades/import/csv${qs}`, {
          method: 'POST',
          headers,
          body: blob,
        });
      } catch (e) {
        throw new ApiError(0, `Réseau injoignable (${base}). Vérifie l'URL et le VPN.`);
      }
      return parseResponse<ImportSummary>(res);
    },
  };
}

export type Api = ReturnType<typeof createApi>;
