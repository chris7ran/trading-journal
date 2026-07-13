# Sprint 20 — Calendrier : impact faible (jaune) inclus

**Date:** 2026-07-05
**Objectif:** afficher aussi les events à **faible impact** (jaune) du calendrier
Forex Factory, pour que le JPY et les publications de second rang apparaissent.
**Statut:** livré. ⚠️ **Backend recompilé requis** + recharge mobile.

---

## Contexte

Le Forecast et le Previous étaient **déjà** parsés depuis Forex Factory (source
`ff_calendar_thisweek.json`) et affichés (liste + fiche macro). Mais le backend ne
gardait que High (rouge) et Medium (orange) → beaucoup d'events JPY (jaunes) étaient
filtrés.

## Changements

- `backend/src/routes/market.rs` : le parseur mappe désormais `low => "yellow"`
  (au lieu de l'ignorer). Les jours fériés / lignes non-éco restent exclus.
- `mobile/src/api/types.ts` : `EcoEvent.impact` = `'red' | 'orange' | 'yellow'`.
- `mobile/src/screens/CalendarScreen.tsx` : point de couleur — rouge (High),
  ambre (Medium), **gris discret** (Low) pour ne pas surcharger ; message vide
  simplifié.

Le filtre par actif (dont JPY) permet de réduire le bruit ; l'estimé de la fiche
macro bénéficie aussi de ces events supplémentaires.

## Déploiement

```
cd code/backend && cargo run   # recompile
# mobile : r
```

## Vérification

Relecture statique : changement minime (mapping impact + type + couleur du point).
Aucun autre code ne dépend de l'ancien schéma d'impact (l'écran MacroScreen legacy
n'est pas monté).
