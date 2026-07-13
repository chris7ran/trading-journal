# Sprint 22 — Marché intégré aux indicateurs macro (or, WTI, taux US)

**Date:** 2026-07-05
**Objectif:** ajouter or, pétrole (WTI), rendement 10 ans US et proxy Fed (2 ans US)
dans les cartes macro de l'onglet Économie, avec une fiche détail adaptée.
**Statut:** livré. ⚠️ **Backend recompilé requis** + recharge mobile.

---

## Backend

- `models.rs` : `EconIndicator` gagne `category: String` (`"macro"` | `"market"`).
- `routes/market.rs` :
  - `MARKETS` (label, region, symbole Stooq, unité) : Or (xauusd, $), Pétrole
    WTI (cl.f, $), 10 ans US (10usy.b, %), Fed/2 ans US (2usy.b, %).
  - `fetch_markets()` : CSV quotidien Stooq (gratuit, sans clé, EOD), parse la
    colonne Close, garde ~30 séances (value = dernière, previous = veille,
    history = série). Ignore proprement toute source injoignable.
  - `economy()` concatène macro (World Bank) + marché ; cache + stale inchangés.

Le vrai **taux directeur Fed** n'existe pas en gratuit sans clé → on utilise le
**2 ans US** comme proxy des anticipations Fed (labellisé « Fed / 2 ans US (proxy) »).

## Mobile

- `api/types.ts` : `EconIndicator.category: 'macro' | 'market'`.
- `utils/macro.ts` : `isMarket(ind)` + `marketReflection(ind)` (chaîne causale par
  instrument : or = refuge/taux réels/dollar ; WTI = offre-demande/inflation ;
  10 ans = taux de référence ; 2 ans = anticipations Fed + pente 10-2).
- `components/MacroDetail.tsx` : si marché → entête « vs séance préc. » + section
  « Lecture du marché » (pas de scénarios « estimé », pas d'historique annuel).
  Si macro → inchangé (constat, scénarios, lecture, historique annuel).
- `screens/CalendarScreen.tsx` : la carte affiche le delta « · séance » pour le
  marché (au lieu de « vs année-1 »), décimales adaptées (2 pour les %).

## Déploiement

```
cd code/backend && cargo run   # recompile + nouvelles cartes marché
# mobile : r
```

## Notes / limites

- Données marché = **fin de journée** (EOD), pas temps réel.
- Dépend de l'accès réseau du serveur à Stooq ; si injoignable, les cartes
  marché n'apparaissent pas (les cartes World Bank restent).
- Vrai Fed Funds → nécessiterait une source à clé (FRED) : à brancher plus tard.

## Vérification

Relecture statique backend (Rust) + mobile (TS).
