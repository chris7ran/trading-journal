# Sprint 16 — Dashboard interactif (façon AccuTrader) + setups modèles

**Date:** 2026-07-04
**Objectif:** ajouter au Dashboard des blocs interactifs inspirés d'AccuTrader
(on tape → détails), et proposer 2 stratégies prêtes à l'emploi dans Setups.
**Statut:** livré, **100% mobile** → recharger (`r`).

---

## Nouveaux blocs Dashboard (`components/DashboardExtras.tsx`)

Tout est calculé côté client à partir des trades chargés (aucun endpoint).

- **Trading Health Trends** : courbe de « santé de trading » (fenêtre glissante de
  win-rate + profit factor, score 0..3) colorée par zone
  (OutPerforming→Good→Caution→Reassess) + toggle marche/arrêt.
- **Performance by Day & Time** : chips jour (All/Lun…Dim) + barres par créneau de
  30 min (vert au-dessus / rouge en dessous d'une ligne centrale). **Tape une
  barre** → Avg Profit / Win Rate / créneau (ex: 08:30 PM - 09:00 PM).
- **Top Performing Setups** : barres win-rate par setup. **Tape** → Pattern / Win
  Rate / Net Profit.
- **Top Symbols Traded** : top 5 symboles par profit cumulé (▲/▼).

`utils/analytics.ts` : ajout de `timeSlotPerformance`, `symbolPerformance`,
`healthSeries`. L'ancien bloc « Par instrument » est remplacé par Top Symbols.

## Setups — modèles 1-tap (`screens/SetupsScreen.tsx`)

Section « Modèles de stratégie » avec 2 stratégies pré-rédigées, ajoutables d'un
tap (`createSetup`), puis assignables aux trades :

- **Gold Reversal Sniper** (XAUUSD) — d'après le PDF Gold Code Academy : engulfing
  H4 sur zone, Order Block M5 + correction harmonique, entrée M1
  (accumulation → balayage liquidité → BOS → pullback OB/FVG), cibles/RR.
- **ORB (Opening Range Breakout)** — range d'ouverture, cassure en clôture, stop
  opposé, cible 1-2x le range.

Le bouton passe à « Déjà ajouté ✓ » si un setup du même nom existe.

## Vérification

⚠️ Non lancé ici. Relecture statique. Mobile only → `r`.

## Reste à faire / prochaines étapes (demandé par Chris via captures)

- [ ] **Fiche Setup détaillée** (façon AccuTrader) : liste avec anneau win-rate +
      total trades + reliability + last used ; page détail avec grille de metrics,
      « Performance over time », « Performance by Day & Time », notes
      « When it worked / didn't work ». Les stats sont calculables côté client,
      mais les champs structurés (Target Entry / Target Exit / Stop Loss /
      Description) nécessitent une **migration backend** (nouvelles colonnes) →
      à cadrer avec Chris.
