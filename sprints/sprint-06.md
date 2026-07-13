# Sprint 6 — Dashboard unifié, Macro séparé + sélecteur d'actif, sources

**Date:** 2026-06-29
**Objectif:** fusionner Stats dans le Dashboard, séparer Calendrier éco et News
en deux onglets avec un sélecteur d'actif, et améliorer les sources de données.
**Statut:** code livré ; backend à recompiler, app à recharger.

---

## Recherche sources (agent finance)

- **Calendrier** : confirmer faireconomy JSON (`ff_calendar_thisweek.json`) comme
  source primaire + **fallback CDN** (`cdn-nfs.faireconomy.media`). Champ `country`
  = devise (filtre). Garde-fou : détecter la page « Request Denied » (non-JSON).
- **News** : flux RSS publics investing.com par thème — `news_1` (Forex),
  `news_95` (indicateurs macro), `news_25` (indices) — + **MarketWatch**
  (`mw_topstories`). Association à l'actif par **mots-clés** (RSS = pas de ticker).
- Pistes à clé (non câblées) : Finnhub (60 req/min, le meilleur free tier),
  Marketaux (filtre `entity_types=index/currency`). Alpha Vantage à éviter (25/j).

## Mobile

- **Dashboard unifié** : intègre désormais le **calendrier P&L** et le **breakdown
  par instrument** (ex-onglet Stats). L'onglet Stats est supprimé.
- **Macro séparé** : deux onglets distincts **Calendrier éco** et **News**.
- **Sélecteur d'actif** (`components/AssetBar.tsx`, `utils/assets.ts`) : chips
  Tout / USD / EUR / US30 / DAX40. Filtre le calendrier par **devise** et les news
  par **mots-clés** (US30→Dow/Nasdaq/S&P, DAX40→DAX/Germany, USD→Fed/dollar, EUR→ECB/euro).
- Onglets : Dashboard · Journal · Prop Firm · Calendrier · News.

## Backend

- `src/routes/market.rs` : feeds RSS mis à jour (investing news_1/95/25 + MarketWatch),
  calendrier avec **fallback CDN** + détection réponse non-JSON.

## Vérification

⚠️ Non compilé/lancé ici. Code relu statiquement. Calendrier/news = réseau runtime
côté serveur — à valider après recompile. Les écrans `StatsScreen`/`MacroScreen`
ne sont plus référencés (remplacés) ; sans impact (non bundlés).

## Reste à faire / prochaines étapes

- [ ] Recompiler backend + recharger app ; valider Dashboard unifié, filtres actif.
- [ ] (Option) Finnhub en source d'appoint (clé gratuite) pour news taggées.
- [ ] Cache calendar/news en base + refresh planifié.
- [ ] Coach IA (Anthropic) — rapport de session + questions post-trade.
