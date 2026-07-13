# Sprint 19 — Fiche détail indicateur macro + Yen (JPY)

**Date:** 2026-07-05
**Objectif:** au tap sur une carte macro (Éco), afficher un historique + variation
+ estimé + une lecture de sentiment ; et ajouter le Yen (JPY) partout dans l'Éco.
**Statut:** livré. ⚠️ **Backend recompilé requis** (indicateurs Japon) + recharge mobile.

---

## Fiche détail indicateur macro

- `utils/macro.ts` : `indicatorYears` (reconstruit les années de l'historique),
  `variation` (abs + %), `matchingEvent` (retrouve la publication à venir
  correspondante dans le calendrier — même devise + mot-clé CPI/emploi/GDP),
  `sentimentText` (lecture règle-basée : inflation ↑ → hawkish/soutien devise ;
  chômage ↑ → dovish ; PIB ↑ → positif, etc.).
- `components/MacroDetail.tsx` : bottom-sheet ouvert au tap sur une carte :
  valeur courante, variation vs an dernier (abs + %), sparkline, **Prochaine
  publication** (estimé + précédent tirés du calendrier), **Lecture / sentiment**,
  et **Historique annuel** (année → valeur + delta). Disclaimer sur les sources.
- `screens/CalendarScreen.tsx` : les cartes macro deviennent cliquables →
  ouvrent la fiche (les événements du calendrier sont passés pour l'estimé).

Note : les données World Bank sont **annuelles** → l'historique et la variation
sont annuels ; l'« estimé » officiel provient du calendrier éco de la semaine
quand une publication correspondante est prévue.

## Yen (JPY)

- `utils/assets.ts` : nouvel actif **JPY** (devise JPY + mots-clés yen/boj/ueda/
  japan/nikkei…) → filtre le calendrier et les news.
- Backend `routes/market.rs` : ajout des indicateurs **Japon** (JPN) — Inflation
  (CPI), Chômage, Croissance PIB (World Bank). Ils apparaissent dans les cartes
  macro et le filtre JPY.

## Déploiement

```
cd code/backend && cargo run   # recompile : ajoute les indicateurs Japon
# mobile : r
```

## Vérification

⚠️ Non lancé ici. Relecture statique backend + mobile.

## Reste à faire / idées

- [ ] Granularité mensuelle (FRED/DBnomics) pour un vrai « mois dernier vs mois
      il y a un an » (World Bank ne fournit que l'annuel).
- [ ] Ajouter d'autres régions/paires au besoin.
