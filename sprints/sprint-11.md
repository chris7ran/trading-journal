# Sprint 11 — Setups + revue de trade (étape 2) & retouches Journal

**Date:** 2026-06-29
**Objectif:** remplacer l'onglet Prop Firm par **Setups**, ajouter la **revue de
trade** (setup + questions), + retouches Journal (menu «+», calendrier cliquable).
Inspiré d'AccuTrader.
**Statut:** livré ; **backend à recompiler** (migration 0002), app à recharger.

---

## Backend

- **Migration 0002** : table `setups` (id, name, rules) + colonnes de revue sur
  `trades` : `followed_plan`, `respected_sl`, `pattern_valid`, `thesis_worked`,
  `good_exit` (INTEGER 1/0/NULL). Le setup choisi est stocké dans `setup_tag`.
- Modèles : `Trade` + `NewTrade`/`UpdateTrade` étendus ; `Setup`/`NewSetup`.
- `GET/POST /setups` (`routes/setups.rs`). INSERT/UPDATE trades incluent la revue.

## Mobile

- **Onglet Setups** (remplace Prop Firm) : liste + création de setups (nom + règles).
- **Carte de trade** (`TradeForm`) : sélecteur de **setup** (chips depuis /setups) +
  section **Revue** — 5 questions Oui/Non/— (tri-état) : plan suivi, SL respecté,
  pattern valide, thèse validée, sortie maîtrisée. `TradeDetail` affiche la revue (✓/✗/—).
- **Journal** : bouton d'en-tête **« + »** → menu (Importer un fichier MT5 / Saisie
  manuelle) au lieu de « + Trade » + gros bouton import. **Calendrier P&L** cliquable :
  tap sur un jour → scroll vers le premier trade de ce jour (`scrollToIndex` + fallback).
- Prop firm : tracking déjà déplacé en carte Dashboard (sprint 10) ; la config des
  règles prop firm reste (temporairement) hors app — à reloger si besoin.

## Questions de revue (recherche)

Basées sur les checklists TradeZella/AccuTrader/psycho trading : adhérence au plan,
respect du risque (SL), validité du setup, thèse, qualité d'exécution. Fondation
du futur **top mistakes** (règles le plus souvent enfreintes).

## Vérification

⚠️ Non lancé ici. Relecture statique. **Recompiler** le backend (migration 0002
s'applique automatiquement, colonnes ajoutées aux données existantes en NULL).

## Reste à faire / prochaines étapes

- [ ] Recompiler backend + recharger app ; créer un setup, tagger un trade, vérifier la revue.
- [ ] **Top mistakes** + **performance par setup** (analytics à partir des champs de revue).
- [ ] Reloger la config des règles prop firm (dans Setups ?).
