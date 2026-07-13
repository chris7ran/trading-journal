# Sprint 26 — Éco : sections repliables + mise en valeur des chiffres phares

**Date:** 2026-07-06
**Statut:** livré, **100% mobile** → recharger (`r`).

---

## Sections repliables

`screens/CalendarScreen.tsx` : les blocs **« Indicateurs macro »** et
**« Calendrier éco »** ont un en-tête cliquable avec chevron ▾/▸ (states
`openMacro` / `openCal`) → on replie/déplie pour cacher ce qu'on ne veut pas voir.
S'ajoute au regroupement par actif + par type (Mensuel / Marché / Annuel) déjà en
place.

## Mise en valeur des chiffres phares imminents

Logique finance appliquée côté app (pas de connecteur externe) : une carte
indicateur s'illumine quand une **publication à fort/moyen impact** correspondante
arrive **sous 10 jours** (CPI, emploi, PIB, décision de taux — ce qui bouge les
marchés).

- `utils/macro.ts` : `daysUntil`, `indicatorHighlight(ind, events)` (relie
  l'indicateur à sa prochaine publication via `matchingEvent`, garde red/orange à
  ≤ 10 j), `countdownLabel` (« auj. » / « demain » / « dans Nj »).
- `CalendarScreen` : carte surlignée (bordure rouge si fort impact, ambre si moyen)
  + badge **📅 dans Nj** en haut à droite. On repère d'un coup d'œil ce qui sort
  bientôt (ex. la carte « Inflation US » s'allume quand le CPI approche).

Le bandeau « ⚠︎ À venir · fort impact » (sprint 24) complète : les 3 prochaines
publications rouges.

## Note

Le matching indicateur→publication s'appuie sur les titres anglais du calendrier
Forex Factory (CPI, Unemployment, GDP) — c'est le cas de la source, donc OK.

## Vérification

Relecture statique (subagent) : aucun bloquant. Mobile only → `r`.
