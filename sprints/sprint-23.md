# Sprint 23 — Filtre d'impact calendrier + diagnostic cartes marché

**Date:** 2026-07-05
**Statut:** livré. Filtre = mobile (`r`). Logs marché = backend (`cargo run`).

---

## Filtre d'impact (mobile)

`screens/CalendarScreen.tsx` : rangée de puces **Fort (rouge) / Moyen (orange) /
Faible (jaune)** au-dessus du calendrier, chacune activable. Par défaut **Fort +
Moyen** actifs, **Faible désactivé** → plus d'events gris/faibles par défaut ;
Chris active « Faible » s'il veut tout voir. Filtre appliqué en plus du filtre par
actif.

## Cartes marché qui n'apparaissent pas — diagnostic

Les cartes Or/WTI/Taux viennent de Stooq (backend). Deux causes possibles :
1. **Backend non recompilé** (ce sprint et le précédent nécessitent `cargo run`,
   pas seulement `r`). Sans recompilation, l'API ne renvoie pas ces cartes.
2. **Stooq injoignable** depuis le serveur.

Ajout de logs dans `routes/market.rs` → `fetch_markets` :
- `tracing::warn!` par symbole en cas d'échec requête / corps / réponse vide.
- `tracing::info!(count=…)` du nombre d'instruments récupérés.

Au démarrage/refresh, la console backend affichera par ex.
`marché: instruments récupérés (Stooq) count=4` (ok) ou des `warn` par symbole
(injoignable). Ça permet de trancher entre « pas recompilé » et « réseau bloqué ».

## À faire côté Chris

```
cd code/backend && cargo run     # recompile + logs marché
# mobile : r
```
Regarder la console backend : si `count=0` + warns → Stooq bloqué (on passera à une
autre source ou FRED à clé). Si `count=4` → les cartes doivent apparaître.
