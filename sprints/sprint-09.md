# Sprint 9 — Économie globale (World Bank) + sentiment des news

**Date:** 2026-06-29
**Objectif:** inspiré de Fincept Terminal (sans forker — licence AGPL+commerciale
hostile, stack C++/Qt incompatible), refaire une base **fondamentale** en propre :
indicateurs macro ouverts + une nouveauté, le **sentiment**.
**Statut:** code livré ; backend à recompiler, app à recharger.

---

## Positionnement Fincept

Fincept Terminal = super source d'inspiration (Bloomberg-like), mais **non
intégrable** : desktop C++20/Qt6 + Python, et licence AGPL-3.0 + commerciale très
restrictive (fork/usage pro interdits sans licence payante). On garde les **idées**
et les **sources ouvertes** (World Bank / FRED / DBnomics), refaites nativement.

## Économie (World Bank, sans clé)

**Backend** (`market.rs`, modèle `EconIndicator`)
- `GET /macro/economy` : indicateurs via l'API World Bank (annuel, sans clé) —
  Inflation/Chômage/PIB pour **US, zone euro, Allemagne** (codes `FP.CPI.TOTL.ZG`,
  `SL.UEM.TOTL.ZS`, `NY.GDP.MKTP.KD.ZG`). Retourne valeur récente, précédente, et
  un **historique** (pour sparkline). Cache 12 h. Sources en panne = ignorées.

**Mobile** — onglet **« Éco »** (ex-Calendrier, fusionné) :
- Cartes d'indicateurs avec **mini-courbes** (`Sparkline`, react-native-svg) +
  delta vs année précédente ; au-dessus du calendrier éco existant.
- Piste future facile : FRED (mensuel, clé gratuite) / DBnomics (Eurostat/BCE).

## Sentiment des news (nouveauté)

**Backend** — `sentiment_of(title)` : lexique bullish/bearish → champ `sentiment`
(`bullish`/`bearish`/`neutral`) sur chaque `NewsItem`.

**Mobile** — écran **News** :
- **Pastille de sentiment** par article (vert/rouge/gris).
- **Jauge « humeur du marché »** : part de titres haussiers vs baissiers, calculée
  sur les news filtrées par l'actif sélectionné (USD/EUR/US30/DAX40).

## Navigation

Onglets : Dashboard · Journal · Prop Firm · **Éco** · News · Coach (6).

## Vérification

⚠️ Non lancé ici. Relecture statique. Backend à recompiler (appels World Bank au
runtime, cachés 12 h). Données World Bank = annuelles (contexte de fond, pas temps réel).

## Reste à faire / prochaines étapes

- [ ] Recompiler backend + recharger app ; vérifier Éco (indicateurs + sparklines) et sentiment.
- [ ] FRED/DBnomics pour de la granularité mensuelle.
- [ ] Affiner le lexique de sentiment (fr/en) selon le rendu réel.
