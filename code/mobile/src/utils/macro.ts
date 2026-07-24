// Helpers for the macro-indicator detail view: yearly history, variation,
// matching upcoming release (from the economic calendar), and a rules-based
// sentiment reading.

import type { EconIndicator, EcoEvent } from '../api/types';

/** Reconstruct the year for each history point (history is oldest -> newest). */
export function indicatorYears(ind: EconIndicator): number[] {
  const y = Number(ind.year);
  const n = ind.history.length;
  if (!Number.isFinite(y) || n === 0) return [];
  return ind.history.map((_, i) => y - (n - 1 - i));
}

export function variation(ind: EconIndicator): { abs: number; pct: number | null } | null {
  if (ind.previous == null) return null;
  const abs = ind.value - ind.previous;
  const pct = ind.previous !== 0 ? (abs / Math.abs(ind.previous)) * 100 : null;
  return { abs, pct };
}

type Kind = 'inflation' | 'unemployment' | 'gdp' | 'other';

function kindOf(label: string): Kind {
  const l = label.toLowerCase();
  if (l.includes('inflation') || l.includes('cpi')) return 'inflation';
  if (l.includes('chôm') || l.includes('chom') || l.includes('unemploy')) return 'unemployment';
  if (l.includes('pib') || l.includes('gdp') || l.includes('croiss')) return 'gdp';
  return 'other';
}

const REGION_CCY: Record<string, string> = {
  'États-Unis': 'USD',
  'Zone euro': 'EUR',
  Allemagne: 'EUR',
  'Royaume-Uni': 'GBP',
  Japon: 'JPY',
};

function keywordsFor(label: string): string[] {
  switch (kindOf(label)) {
    case 'inflation':
      return ['cpi', 'inflation'];
    case 'unemployment':
      return ['unemployment', 'jobless', 'employment'];
    case 'gdp':
      return ['gdp'];
    default:
      return [];
  }
}

/** Nearest upcoming calendar release matching this indicator (for the estimate). */
export function matchingEvent(ind: EconIndicator, events: EcoEvent[]): EcoEvent | null {
  const ccy = REGION_CCY[ind.region];
  if (!ccy) return null;
  const kws = keywordsFor(ind.label);
  if (kws.length === 0) return null;
  const matches = events
    .filter((e) => e.currency === ccy && kws.some((k) => e.title.toLowerCase().includes(k)))
    .sort((a, b) => a.date.localeCompare(b.date));
  return matches[0] ?? null;
}

/** Extract the first signed decimal number from a Forex Factory metric string ("2.9%", "7.28M", "3.86|2.9"). */
export function parseMetric(s: string | null): number | null {
  if (s == null) return null;
  const m = s.replace(/,/g, '.').match(/-?\d+(\.\d+)?/);
  if (!m) return null;
  const n = parseFloat(m[0]);
  return Number.isFinite(n) ? n : null;
}

/** Currency code for a region label, or null when unknown. */
export function regionCurrency(region: string): string | null {
  return REGION_CCY[region] ?? null;
}

export interface MacroCommentary {
  constat: string;
  scenarios: { label: string; text: string }[];
  baseCase: string;
}

/** Equity indices reference by region, for the reasoning chain. */
function indicesFor(region: string): string {
  if (region === 'États-Unis') return 'les indices US (US30, Nasdaq, S&P 500)';
  if (region === 'Zone euro' || region === 'Allemagne') return 'les indices européens (DAX, CAC)';
  if (region === 'Royaume-Uni') return 'le FTSE 100';
  if (region === 'Japon') return 'le Nikkei';
  return 'les indices actions';
}

/** Expected market reaction by indicator type when the release surprises. */
function reaction(kind: Kind, outcome: 'above' | 'inline' | 'below', ccy: string): string {
  switch (kind) {
    case 'inflation':
      if (outcome === 'above')
        return `Inflation qui surprend à la hausse : le marché price des taux plus fermes → rendements et ${ccy} en hausse, pression immédiate sur l'or et les indices actions.`;
      if (outcome === 'below')
        return `Désinflation plus rapide qu'attendu : espoir de baisses de taux → ${ccy} et rendements en baisse, rebond des actions et de l'or.`;
      return `Conforme à l'estimé : peu de mouvement direct sur le ${ccy} ; le marché se concentre sur le cœur de l'inflation (core, services, loyers) pour la tendance de fond.`;
    case 'unemployment':
      if (outcome === 'above')
        return `Chômage plus élevé que prévu (emploi faible) : anticipation d'assouplissement → ${ccy} sous pression, mais les actions peuvent tenir si le marché espère des taux plus bas.`;
      if (outcome === 'below')
        return `Chômage plus bas que prévu (emploi solide) : biais restrictif renforcé → ${ccy} en hausse, prudence sur les valeurs sensibles aux taux.`;
      return `Conforme : réaction limitée ; regarde les salaires et le taux de participation pour la vraie tendance.`;
    case 'gdp':
      if (outcome === 'above')
        return `Croissance au-dessus de l'estimé : demande robuste, appétit pour le risque → ${ccy} et indices soutenus.`;
      if (outcome === 'below')
        return `Croissance sous l'estimé : crainte de ralentissement → ${ccy} et indices sous pression, repli vers obligations et or.`;
      return `Conforme : le marché confirme sa trajectoire, impact modéré.`;
    default:
      if (outcome === 'above') return `Au-dessus de l'estimé : réaction selon le contexte, surveille le ${ccy}.`;
      if (outcome === 'below') return `Sous l'estimé : réaction selon le contexte, surveille le ${ccy}.`;
      return `Conforme : impact généralement limité.`;
  }
}

/** Causal-chain reading of the current level/trend, linking economy, policy and assets. */
function reflection(kind: Kind, annualDir: number, ccy: string, ind: EconIndicator): string {
  const region = ind.region;
  const cur = `${ind.value.toFixed(1)}${ind.unit}`;
  const idx = indicesFor(region);
  switch (kind) {
    case 'inflation':
      if (annualDir < 0)
        return `L'inflation ${region} est à ${cur} et décélère. La désinflation donne à la banque centrale la marge pour assouplir : à terme, des taux plus bas soutiennent moins le ${ccy}, mais soulagent les actions et l'or (coût de portage réduit). Point de vigilance : un rebond des prix (énergie, salaires) relancerait un discours restrictif.`;
      if (annualDir > 0)
        return `L'inflation ${region} est à ${cur} et accélère. La banque centrale est poussée à durcir (taux hauts plus longtemps) : cela soutient le ${ccy} via des rendements attractifs, mais freine le crédit, pèse sur ${idx} et l'or, et finit par ralentir la croissance.`;
      return `L'inflation ${region} est à ${cur}, globalement stable et proche de la cible. Contexte neutre : la banque centrale peut temporiser ; les mouvements viendront surtout des surprises sur les prochaines publications.`;
    case 'gdp':
      if (annualDir > 0)
        return `La croissance ${region} est à ${cur} et accélère. Plus de croissance = plus d'emploi et de pouvoir d'achat → consommation et bénéfices des entreprises en hausse : cercle vertueux favorable à ${idx} et au ${ccy}. Tant que l'inflation reste maîtrisée, la banque centrale peut rester patiente.`;
      if (annualDir < 0)
        return `La croissance ${region} est à ${cur} et ralentit. Moins d'activité = moins d'embauches et une demande plus molle : marges des entreprises sous pression, ${idx} fragilisés, et une banque centrale poussée vers des baisses de taux (négatif pour le ${ccy} à terme). Un PIB durablement faible fait craindre la récession.`;
      return `La croissance ${region} est à ${cur}, stable. Trajectoire de croisière : peu de catalyseur directionnel ; l'attention se porte sur l'inflation et l'emploi pour anticiper la politique monétaire.`;
    case 'unemployment':
      if (annualDir > 0)
        return `Le chômage ${region} est à ${cur} et remonte : le marché du travail se refroidit → moins de revenus → consommation en baisse → risque pour la croissance. La banque centrale tend alors à assouplir (baisses de taux), ce qui affaiblit le ${ccy} mais peut soutenir les actions (anticipation de taux plus bas).`;
      if (annualDir < 0)
        return `Le chômage ${region} est à ${cur} et baisse : marché du travail tendu → pressions salariales → soutien de la consommation, mais aussi de l'inflation. La banque centrale reste vigilante (biais plutôt restrictif), globalement positif pour le ${ccy}.`;
      return `Le chômage ${region} est à ${cur}, stable : marché de l'emploi équilibré ; l'impact viendra des tendances salariales et des créations d'emplois.`;
    default:
      return `${ind.label} ${region} est à ${cur}.`;
  }
}

/** True for market instruments (gold, oil, yields) vs annual macro indicators. */
export function isMarket(ind: EconIndicator): boolean {
  return ind.category === 'market';
}

/** Causal-chain reading for a market instrument (gold, oil, US yields). */
export function marketReflection(ind: EconIndicator): string {
  const l = ind.label.toLowerCase();
  if (l.includes('xau') || ind.region === 'Or')
    return `L'or est une valeur refuge et une couverture contre l'inflation. Il monte quand les taux réels baissent (Fed accommodante, désinflation), quand le dollar faiblit, ou en cas de stress géopolitique. Il baisse quand les rendements réels montent (Fed restrictive) et que le dollar se renforce.`;
  if (l.includes('wti') || l.includes('pétrole') || l.includes('petrole') || l.includes('oil') || ind.region === 'Pétrole')
    return `Le pétrole (WTI) reflète l'offre (OPEP+, stocks US) et la demande mondiale (croissance). Un baril en hausse alimente l'inflation (énergie, transport) et complique la tâche des banques centrales ; une baisse signale souvent un ralentissement de la demande. C'est à la fois un baromètre de croissance et un moteur d'inflation.`;
  if (l.includes('10'))
    return `Le rendement du 10 ans US est le taux de référence mondial. Il monte quand le marché anticipe croissance/inflation ou une Fed restrictive → soutient le dollar mais pèse sur l'or et les valeurs de croissance. Il baisse lors d'une fuite vers la sécurité ou d'anticipations de baisses de taux.`;
  if (l.includes('2') || l.includes('fed'))
    return `Le 2 ans US suit de près les anticipations de politique de la Fed (proxy du taux directeur). En hausse = marché qui price des taux plus hauts/longtemps (hawkish, dollar soutenu) ; en baisse = anticipation de baisses (dovish, dollar sous pression). L'écart 10 ans − 2 ans (pente de la courbe) signale les craintes de récession quand il devient négatif.`;
  return `${ind.label} : instrument de marché ; suis la tendance récente et le contexte macro.`;
}

/** Structured commentary: current read, surprise scenarios, and a causal-chain reading. */
export function macroCommentary(ind: EconIndicator, event: EcoEvent | null): MacroCommentary {
  const ccy = regionCurrency(ind.region) ?? ind.region;
  const kind = kindOf(ind.label);

  const v = variation(ind);
  const annualDir = v == null ? 0 : v.abs > 0.05 ? 1 : v.abs < -0.05 ? -1 : 0;
  const prevYear = Number(ind.year) - 1;
  const cur = `${ind.value.toFixed(1)}${ind.unit}`;
  const prevY = ind.previous != null ? `${ind.previous.toFixed(1)}${ind.unit}` : '—';

  const monthly = ind.category === 'macro_monthly';
  const dirWord = annualDir > 0 ? 'en hausse' : annualDir < 0 ? 'en baisse' : 'stable';
  let constat = monthly
    ? `${ind.label} (${ind.region}) ressort à ${cur}, ${dirWord} vs le mois dernier (${prevY}).`
    : `${ind.label} (${ind.region}) ressort à ${cur}, ${dirWord} sur un an (contre ${prevY} en ${prevYear}).`;
  if (event) {
    constat += ` Prochaine publication : le consensus attend ${event.forecast ?? '—'} contre ${event.previous ?? '—'} le mois dernier.`;
  }

  const scenarios = [
    { label: `Chiffre au-dessus de l'estimé`, text: reaction(kind, 'above', ccy) },
    { label: `Chiffre conforme à l'estimé`, text: reaction(kind, 'inline', ccy) },
    { label: `Chiffre sous l'estimé`, text: reaction(kind, 'below', ccy) },
  ];

  const baseCase = reflection(kind, annualDir, ccy, ind);

  return { constat, scenarios, baseCase };
}

/** Whole days from now until an ISO date (negative if past). */
export function daysUntil(iso: string): number | null {
  const d = new Date(iso.replace(' ', 'T')).getTime();
  if (isNaN(d)) return null;
  return Math.ceil((d - Date.now()) / 86_400_000);
}

export interface IndicatorHighlight {
  event: EcoEvent;
  days: number; // 0 = today
  strong: boolean; // true for red (high) impact
}

/**
 * Flag a "hot" indicator: it has a matching HIGH/medium-impact release coming up
 * within ~10 days — the figures traders watch (CPI, jobs, GDP, rate decisions).
 */
export function indicatorHighlight(ind: EconIndicator, events: EcoEvent[]): IndicatorHighlight | null {
  const ev = matchingEvent(ind, events);
  if (!ev) return null;
  if (ev.impact !== 'red' && ev.impact !== 'orange') return null;
  const d = daysUntil(ev.date);
  if (d === null || d < 0 || d > 10) return null;
  return { event: ev, days: d, strong: ev.impact === 'red' };
}

/** Short human label for a countdown ("auj.", "demain", "dans 3j"). */
export function countdownLabel(days: number): string {
  if (days <= 0) return "auj.";
  if (days === 1) return 'demain';
  return `dans ${days}j`;
}

/** Rules-based sentiment reading based on the year-over-year direction. */
export function sentimentText(ind: EconIndicator): string {
  const v = variation(ind);
  const dir = v == null ? 0 : v.abs > 0.05 ? 1 : v.abs < -0.05 ? -1 : 0;
  const prevYear = Number(ind.year) - 1;
  const cur = `${ind.value.toFixed(1)}${ind.unit}`;
  const prev = ind.previous != null ? `${ind.previous.toFixed(1)}${ind.unit}` : '—';
  const move =
    dir > 0
      ? `plus élevé qu'en ${prevYear} (${cur} contre ${prev})`
      : dir < 0
        ? `plus bas qu'en ${prevYear} (${cur} contre ${prev})`
        : `quasi stable (${cur})`;

  switch (kindOf(ind.label)) {
    case 'inflation':
      if (dir > 0)
        return `L'indice est ${move}. Une inflation qui accélère pousse la banque centrale vers une politique plus restrictive (taux hauts plus longtemps) : globalement un soutien pour la devise, mais une pression sur les actions et souvent sur l'or. Un chiffre publié au-dessus de l'estimé renforce ce biais « hawkish ».`;
      if (dir < 0)
        return `L'indice est ${move}. Un ralentissement de l'inflation ouvre la voie à une politique plus accommodante (baisses de taux possibles) : plutôt négatif pour la devise, favorable aux actions. Un chiffre sous l'estimé accentue le biais « dovish ».`;
      return `L'indice est ${move}. Sans écart marqué, l'impact directionnel est limité : surveille surtout l'écart entre le chiffre publié et l'estimé pour lire le sentiment.`;
    case 'unemployment':
      if (dir > 0)
        return `Le chômage est ${move}. Un marché de l'emploi qui se dégrade pousse la banque centrale vers plus d'accommodation : plutôt négatif pour la devise. Un chiffre au-dessus de l'estimé renforce le biais « dovish ».`;
      if (dir < 0)
        return `Le chômage est ${move}. Un marché de l'emploi solide soutient une politique restrictive : plutôt positif pour la devise. Un chiffre sous l'estimé renforce le biais « hawkish ».`;
      return `Le chômage est ${move}. Impact directionnel limité : l'écart avec l'estimé donnera le ton.`;
    case 'gdp':
      if (dir > 0)
        return `La croissance est ${move}. Une économie qui accélère est positive pour la devise et les actifs risqués. Un chiffre au-dessus de l'estimé renforce le sentiment positif.`;
      if (dir < 0)
        return `La croissance est ${move}. Un ralentissement fait craindre une politique plus accommodante et pèse sur le sentiment. Un chiffre sous l'estimé accentue la prudence.`;
      return `La croissance est ${move}. Impact directionnel limité : compare le chiffre publié à l'estimé.`;
    default:
      return `Valeur ${move}.`;
  }
}
