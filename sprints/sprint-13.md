# Sprint 13 — Filtres analytics + config prop firm relogée

**Date:** 2026-06-29
**Objectif:** filtres compte/période sur les analytics de revue, et reloger la
config des règles prop firm. **100% mobile** (aucun backend → recharger l'app).

---

## Coach = hub analytics (avec filtres)

- Ajout d'un **AccountPicker** + **FilterBar (période)** → `filtersToQuery` passé à
  `listTrades`. Les analytics se recalculent par **compte** et **période**.
- L'écran affiche désormais : insights, **top mistakes**, et **performance par
  setup** (déplacée depuis l'onglet Setups pour centraliser l'analyse).

## Setups = gestion + config prop firm

- **Performance par setup retirée** de Setups (désormais dans Coach).
- Ajout d'une section **« Règles prop firm (par compte) »** : `AccountPicker`
  (compte unique) + solde de départ + DD journalier/global + objectif (%),
  enregistrés via `updateAccount` + `upsertRules`. C'est la config qui manquait
  depuis la suppression de l'ancien onglet Prop Firm ; la carte de statut reste
  sur le Dashboard.

## Vérification

⚠️ Non lancé ici. Relecture statique. Mobile only → `r`. Aucune modif backend
(réutilise `/accounts`, `/accounts/:id/rules`, filtres `/trades`).

## Reste à faire / prochaines étapes

- [ ] Recharger, filtrer le Coach (compte/période), configurer les règles d'un compte.
- [ ] (Option) Graphique de « trading health » dans le temps (AccuTrader).
