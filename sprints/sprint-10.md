# Sprint 10 — Refonte Dashboard façon AccuTrader (étape 1/2)

**Date:** 2026-06-29
**Objectif:** améliorer le Dashboard (inspiré d'AccuTrader) et réorganiser, à
partir des retours de test. **Étape 1 = 100% mobile** (aucun backend → juste `r`).
**Statut:** livré ; étape 2 (onglet Setups) à suivre.

---

## Fait (étape 1, mobile only)

**Charts** (`components/Charts.tsx`)
- **Courbe d'equity** : ajout de l'**échelle Y** (max / 0 / min) + gouttière gauche.
  S'adapte déjà au compte / filtre sélectionné.
- **P&L par jour** : barres à **bouts arrondis** (rx 4), **couleurs vives**
  (`greenBright`/`redBright`), + **échelle Y** (+max / 0 / −max).
- Nouveau `Sparkline` (déjà utilisé par l'éco).

**Dashboard** (`screens/DashboardScreen.tsx`)
- Ajout **Performance par durée** (< 5 min / 5–30 min / Intraday / Swing >1j) et
  **Type de trades** (LONG/SHORT/…, prêt pour CALL/PUT) — `utils/analytics.ts`.
- Ajout **carte Prop firm compacte** (`components/PropFirmCard.tsx`) : objectif,
  drawdown global, consistance, pour le compte sélectionné (rien si « Tous » ou
  pas de règles). Réutilise `computePropStatus`.
- **Calendrier P&L retiré** du Dashboard.

**Journal** (`screens/TradesScreen.tsx`)
- **Calendrier P&L** déplacé ici (en tête de liste) pour repérage rapide par date.

**Calendrier** (`components/CalendarHeatmap.tsx`)
- **Recentré** : cellules `100/7 %` sans marges en % (qui débordaient), espacement
  via padding interne — grille alignée.

## À venir (étape 2)

- Onglet **Setups** (remplace Prop Firm) : setups + checklist de règles + tagging
  à l'import + **top mistakes** (backend : tables setups/rules/violations).
- Config des règles prop firm à loger (probablement dans Setups), la carte
  Dashboard restant l'affichage.

## Vérification

⚠️ Non lancé ici. Relecture statique. Étape 1 = mobile only → recharger l'app (`r`).
