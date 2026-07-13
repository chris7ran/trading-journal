# Sprint 17 — Fiche Setup détaillée (façon AccuTrader)

**Date:** 2026-07-04
**Objectif:** liste de setups enrichie (anneau win-rate + stats) et page détail par
setup avec metrics, performance dans le temps, perf par jour/heure, et champs
structurés éditables (Description, Target Entry/Exit, Stop Loss, Règles).
**Statut:** livré. ⚠️ **Backend recompilé requis** (nouvelle migration + endpoint) +
recharge mobile.

---

## Backend

- `migrations/0003_setup_fields.sql` : `ALTER TABLE setups` +
  `description, target_entry, target_exit, stop_loss` (TEXT).
- `models.rs` : `Setup` gagne ces 4 champs ; `NewSetup` les accepte (optionnels,
  `#[serde(default)]`) ; nouveau `UpdateSetup` (tous optionnels).
- `routes/setups.rs` : `create_setup` insère les nouveaux champs ; nouveau
  `update_setup` (`PUT /setups/:id`, `COALESCE(?, col)` → un champ omis garde sa
  valeur) → renvoie le setup à jour.
- `routes/mod.rs` : route `PUT /setups/:id`.

## Mobile

- `api/types.ts` : `Setup`/`NewSetup` étendus + `UpdateSetup`.
- `api/client.ts` : `updateSetup(id, body)`.
- `utils/analytics.ts` : `setupStats(trades, name)` (win/loss rate, total P&L,
  avg win/loss, profit factor, **reliability** = win-rate lissé Laplace,
  **lastUsed**) + `tradesForSetup`.
- `components/WinRing.tsx` : anneau (donut) SVG du win-rate.
- `screens/SetupsScreen.tsx` : cartes cliquables avec anneau + win rate + nb
  trades + P&L + reliability + dernier usage → ouvrent la fiche détail. (Charge
  aussi les trades pour calculer les stats.)
- `screens/SetupDetailScreen.tsx` (nouveau) : grille de metrics (Win/Loss Rate,
  Total Profit, Avg Profit/Loss, Profit Factor), « Performance over time »
  (equity du setup), « Performance by Day & Time » (réutilise le composant), et
  champs éditables Description / Target Entry / Target Exit / Stop Loss / Règles
  avec « Save Changes ». KeyboardAvoidingView (champs jamais sous le clavier).
- `navigation/RootNavigator.tsx` : l'onglet Setups devient un **stack**
  (SetupsList → SetupDetail).

## Déploiement

```
# Backend (serveur/Mac) — recompilation obligatoire (migration + endpoint)
cd code/backend
cargo build --release   # ou cargo run
# la migration 0003 s'applique au démarrage

# Mobile
# recharge l'app (r)
```

## Vérification

⚠️ Non lancé ici. Relecture statique backend (Rust) + mobile (TS).

## Reste à faire / options

- [ ] Notes « When it worked / When it didn't work » (historique) — nécessiterait
      une table de notes par setup.
- [ ] Épingler (pin) un setup en tête de liste.
- [ ] Filtres période (All/1M/1Y) sur la courbe de la fiche.
