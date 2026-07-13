# Sprint 27 — Harmonisation UI : sections repliables + filtres

**Date:** 2026-07-06
**Statut:** livré, **100% mobile** → recharger (`r`).

---

## Composant repliable unifié — `components/Section.tsx`

Un seul composant `Section` pour toute l'app :
- en-tête cliquable (replier/déplier) avec **chevron coloré** (violet `primary2`,
  fini le gris triste) via Ionicons `chevron-down`/`chevron-forward` ;
- `flush` pour les écrans dont le contenu est déjà en marge ;
- `defaultOpen` (replié par défaut pour les blocs secondaires).

Déployé sur **tous les onglets** :
- **Dashboard** : le gros `Switch` de « Trading Health Trends » est remplacé par un
  Section ; « Risque & performance », « Courbe d'equity », « P&L par jour »,
  « Performance by Day & Time », « Top Performing Setups », « Top Symbols »,
  « Performance par durée », « Type de trades » → toutes repliables, style identique.
- **Éco** : « Indicateurs macro » + « Calendrier éco » (remplace mes chevrons gris).
- **Coach** : « Top erreurs », « Performance par setup », « Performance par émotion ».
- **Setups** : « Modèles de stratégie » et « Nouveau setup » (repliés par défaut).

→ Comportement et rendu **cohérents partout** ; chaque rubrique se cache/réduit.

## Filtres du Dashboard refaits — `components/FilterBar.tsx`

Fini le « tout / tous » en vrac :
- **Période** : un vrai **segmented control** (Tout · 30j · 90j · YTD) — un seul
  contrôle propre, l'option active surlignée.
- **Instrument** : un **menu déroulant** (bouton « Tous les instruments ▾ » →
  bottom-sheet avec la liste + coche) au lieu d'une longue rangée de puces.

Cohérent avec le sélecteur de compte (même logique de dropdown).

## Vérification

Relecture statique (subagent, 7 fichiers) : aucun bloquant. Quelques styles
devenus inutilisés (sans impact). Mobile only → `r`.
