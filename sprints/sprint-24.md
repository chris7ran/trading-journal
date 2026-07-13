# Sprint 24 — Risque & performance + lot 1 des améliorations

**Date:** 2026-07-06
**Statut:** livré, **100% mobile** → recharger (`r`). Aucun backend.

---

## Carte « Risque & performance » (Dashboard)

`utils/analytics.ts` : `riskMetrics(trades)` → gain espéré par trade (expectancy),
**max drawdown** (pire creux pic-à-creux du P&L cumulé), meilleur/pire trade,
**série de gains / de pertes** (plus longue), durée moyenne de détention.
`screens/DashboardScreen.tsx` : nouvelle section en grille (affichée s'il y a des
trades).

## Lot 1 des 6 chantiers

1. **Perf par émotion (Coach)** — `emotionBreakdown(trades)` (par `emotion_tag`) +
   section « Performance par émotion » dans le Coach (comme les setups). Renseigne
   l'émotion d'un trade pour voir son impact P&L.
2. **Export CSV (Profil)** — bouton « Exporter mes trades (CSV) » : génère le CSV
   côté app et ouvre la feuille de partage native (aucune lib ajoutée). Note : sur
   iOS le CSV part en texte via le partage ; un vrai fichier `.csv` joint viendra
   avec le lot 2 (expo-sharing) si tu veux.
3. **Alertes in-app (Éco)** — bandeau « ⚠︎ À venir · fort impact » en haut de
   l'onglet Économie : les 3 prochaines publications rouges à venir (selon le
   filtre d'actif). Indépendant du filtre d'impact (toujours visible).

## Reste (lot 2 — prérequis)

- **Screenshots de trades** : joindre une image → `expo-image-picker` (install) +
  champ `screenshot_url` accepté par le backend (recompile).
- **Macro mensuel réel + vrai Fed Funds** : via FRED → nécessite une **clé
  gratuite** (FRED_API_KEY) + recompile.
- **Tests backend** : `cargo test` sur le parser MT5 + les stats (backend).

## Vérification

Relecture statique (auto + subagent) : aucun bloquant. Mobile only → `r`.
