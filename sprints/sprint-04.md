# Sprint 4 — Filtres, tracker prop firm, finition

**Date:** 2026-06-29
**Objectif:** filtres (période + instrument) sur Dashboard/Stats, tracker prop
firm (règles GoatFunded + alertes de drawdown), et finition UI.
**Statut:** code livré ; backend à recompiler, app à recharger.

---

## Backend

- **`PUT /accounts/:id`** (`src/routes/accounts.rs`, modèle `UpdateAccount`) :
  mise à jour partielle (name, balance, currency, is_funded) — sert à fixer le
  **solde de départ** du compte pour le calcul du drawdown. Route ajoutée.
- Les filtres `from`/`to`/`symbol` existaient déjà sur `/trades` et `/trades/stats`.

## Mobile

**Filtres** (`utils/filters.ts`, `components/FilterBar.tsx`)
- Chips période (Tout / 30j / 90j / YTD) + instrument (dérivé des trades).
- Branchés sur **Dashboard** et **Stats** : `filtersToQuery` → `from/to/symbol`
  passés à `getStats` et `listTrades`. La liste d'instruments est stable (grandit,
  ne rétrécit pas). Changement de filtre → rechargement (deps du `useCallback`).

**Tracker prop firm** (`utils/propfirm.ts`, `screens/PropFirmScreen.tsx`, 4e onglet)
- `computePropStatus(trades, startBalance, rule)` : equity cumulée + pic, drawdown
  courant vs limite globale, perte du jour vs limite journalière, progression vers
  l'objectif de profit. Seuils = fractions du solde de départ.
- Écran : en-tête compte/solde/P&L, **alertes** colorées (ok/warn/danger à 80% et
  100% des limites), **barres de progression** (objectif, DD global, DD journalier),
  et un **formulaire** pour régler le solde de départ + les 3 règles (%).
- API client : `listAccounts`, `updateAccount`, `getRules`, `upsertRules` + types
  `Account`/`PropRule`/`UpsertPropRule`/`UpdateAccount`.

**Navigation** : onglets désormais Dashboard / Journal / Stats / Prop Firm (icônes).

## Vérification

⚠️ Non compilé/lancé ici. Code écrit + relu statiquement.
- Backend : recompiler (`cargo run`) pour `PUT /accounts/:id`.
- App : recharger (`r` dans Expo) — pas de nouvelle dépendance native côté mobile.

## Note d'incident (bootstrap)

Lors de l'initialisation Expo, `cp -R mobile-src/App.tsx mobile-src/src mobile/`
n'a copié que `App.tsx` et `src/` ; les fichiers racine d'origine (`README.md`,
`.gitignore`) ont été remplacés par ceux générés par `create-expo-app`. Le
`README.md` mobile a été recréé. Sans impact sur le code (`src/` intact).

## Correctif — /trades/stats 500 sur résultat sans pertes (2026-06-29)

Révélé au test : `GET /trades/stats` renvoyait 500 (`ColumnDecode gross_loss f64
vs INTEGER`) quand le jeu filtré ne contenait **que des trades gagnants** (ex.
filtre `symbol=GER40.x`). `SUM(CASE WHEN pnl<0 THEN pnl ELSE 0 END)` ne sommait
que des `0` entiers → SQLite renvoie un INTEGER, sqlx refuse de le lire en f64.
Fix : `CAST(... AS REAL)` sur toutes les colonnes monétaires (total_pnl, avg_win,
avg_loss, gross_profit, gross_loss, et pnl par symbole). Test de régression ajouté
(`trade_stats_handles_wins_only`).

## Reste à faire / prochaines étapes

- [ ] Recompiler le backend + recharger l'app, valider filtres et prop firm.
- [ ] Min. trading days & règle de consistance dans le tracker.
- [ ] Verrouiller au retour en avant-plan (AppState) pour Face ID.
- [ ] Édition de trade : tags/setups prédéfinis, screenshots.
