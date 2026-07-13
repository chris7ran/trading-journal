# Sprint 8 — Coach (insights à base de règles, gratuit)

**Date:** 2026-06-29
**Objectif:** un « coach » qui analyse les trades et donne des conseils, **sans
API payante** — tout est calculé localement.
**Statut:** code livré (100% côté app, pas de changement backend) ; recharger l'app.

---

## Pourquoi à base de règles

L'API Anthropic (et même un LLM) a un coût / une complexité. Beaucoup de ce qu'un
coach dirait se déduit directement des données → on le calcule en TypeScript,
**gratuit, instantané, privé** (rien ne sort de l'appareil). Conçu derrière une
fonction pure pour pouvoir, plus tard, brancher Ollama (local) ou Anthropic
derrière la même UI.

## Réalisé (100% mobile)

- `utils/coach.ts` — `computeInsights(trades): Insight[]` calcule :
  P&L total + win rate, ratio gain/perte, meilleur/pire instrument (≥3 trades),
  **revenge-trading** (issue du trade après une perte), **consistance** (part du
  meilleur jour), série de pertes max, performance matin vs après-midi, et
  émotion la plus coûteuse (si tags présents). Chaque insight a un niveau
  good/warn/info.
- `screens/CoachScreen.tsx` — onglet **Coach** : `AccountPicker` (par compte ou
  total) + cartes d'insights colorées (vert/rouge/violet), pull-to-refresh.
- Navigation : 6e onglet « Coach » (icône ampoule), labels d'onglets réduits
  (fontSize 10) pour tenir à 6.

## Vérification

⚠️ Non lancé ici. Relecture statique. Aucun changement backend → simple `r` dans Expo.

## Reste à faire / prochaines étapes

- [ ] Recharger l'app, vérifier l'onglet Coach sur tes trades.
- [ ] (Option future) Ollama en local pour des conseils en langage naturel.
- [ ] Affiner les seuils des règles avec le retour d'usage.
