# Sprint 1 — MVP Backend (Rust/axum)

**Date:** 2026-06-20
**Objectif:** poser les fondations du backend self-hosted — CRUD trades, import
CSV MT5, auth JWT, et déploiement Proxmox via systemd/Docker.
**Statut:** code livré, **non compilé** (voir limitation §Vérification).

---

## Périmètre réalisé

| # | Tâche                                   | Statut | Fichiers clés |
|---|-----------------------------------------|--------|---------------|
| 1 | Scaffolding projet Rust/axum            | ✅ | `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/{config,db,state,error}.rs` |
| 2 | SQLite + schema (migrations sqlx)       | ✅ | `migrations/0001_init.sql` |
| 3 | CRUD `/trades`                          | ✅ | `src/routes/trades.rs`, `src/models.rs` |
| 4 | Import `POST /trades/import/csv` (MT5)   | ✅ | `src/csv_import/mt5.rs`, `src/routes/import.rs` |
| 5 | Auth JWT (login → token + middleware)   | ✅ | `src/auth/*` |
| 6 | Dockerfile multi-stage                  | ✅ | `Dockerfile`, `.dockerignore` |
| 7 | Deploy Proxmox via systemd + Tailscale  | ✅ | `deploy/trading-journal-api.service`, `deploy/README.md` |
| 8 | Tests + doc                             | ✅ (tests écrits) | `tests/api.rs`, `src/csv_import/mt5.rs` (unit) |

---

## Décisions de cadrage

1. **Auth = password → JWT.** Pour un single-user self-hosted, c'est le standard :
   mot de passe hashé en Argon2 (jamais en clair, stocké via `ADMIN_PASSWORD_HASH`),
   échangé contre un JWT HS256 (`POST /auth/login`). `/auth/verify-token` reste
   exposé (protégé) pour la validation côté app. Extensible si un 2e compte arrive.

2. **Timestamps en TEXT ISO-8601.** Choix volontaire pour éviter les pièges de
   décodage chrono↔SQLite tant qu'on ne compile pas en local. Typage plus riche
   possible plus tard si l'analytics l'exige.

3. **Parser CSV tolérant.** Mapping par *nom de colonne* (alias, insensible à la
   casse) plutôt que par position fixe, car les exports MT5 varient selon le build
   et le broker. Gère le layout "Positions" (colonnes `Time`/`Price` en double).
   → **À caler sur un vrai export FusionMarkets** (TODO dans `mt5.rs`).

4. **Déduplication par `mt5_ticket`** (contrainte UNIQUE + `ON CONFLICT DO NOTHING`),
   pour réimporter un CSV sans créer de doublons.

5. **Compte par défaut seedé** (`id = 'default'`) en migration, pour que la saisie
   manuelle et l'import marchent sans config préalable.

---

## Écarts vs ARCHITECTURE.md (à valider)

- **`tower-http` 0.5 au lieu de 0.4.** axum 0.7 est passé à l'écosystème http 1.0 ;
  tower-http 0.4 ne compile qu'avec axum 0.6. Écart nécessaire, pas optionnel.
- **Endpoint `/auth/login` ajouté** en plus de `/auth/verify-token` (cf. décision 1).
- Le profil release est optimisé taille (`opt-level="z"`, `lto`, `strip`) pour la
  cible 512 Mo / Raspberry Pi.

---

## Schéma de base

`accounts`, `prop_rules`, `trades`, `sessions` (+ index sur trades et sessions),
conformes à ARCHITECTURE.md. `news_cache` et `eco_events` non inclus dans ce
sprint (Phase 4 — Macro Terminal).

---

## Vérification — IMPORTANT

⚠️ **Le code n'a pas pu être compilé dans l'environnement de travail.** Le sandbox
Linux a son réseau verrouillé par un proxy qui bloque `rust-lang.org`, `crates.io`
et `apt` (HTTP 403). Impossible d'y installer la toolchain Rust ni de récupérer les
crates. Le code a donc été écrit et **relu manuellement** (versions de crates
alignées sur axum 0.7, correction d'un emprunt mutable sur le `QueryBuilder`,
typage CORS), mais la compilation reste **à faire sur la cible**.

### À exécuter sur ta machine / le serveur (réseau ouvert)
```bash
cd code/backend
cargo build            # vérifier la compilation
cargo test             # unit (parser MT5) + intégration (auth/CRUD/import)
cargo clippy           # lint (optionnel mais recommandé)
```
Les tests d'intégration (`tests/api.rs`) montent le router sur une SQLite en
mémoire et couvrent : login OK/KO, 401 sans token, create/list/get, conflit de
ticket dupliqué, import CSV avec dédup au ré-import.

---

## Incrément — endpoints accounts + analytics (2026-06-20, suite)

Ajouts au-delà du périmètre initial du sprint :

| Endpoint | Description | Fichiers |
|----------|-------------|----------|
| `GET /accounts` | liste des comptes | `src/routes/accounts.rs` |
| `POST /accounts` | création (défauts broker/currency/is_funded) | `src/routes/accounts.rs` |
| `GET /accounts/:id/rules` | règles prop firm (404 si absentes) | `src/routes/accounts.rs` |
| `PUT /accounts/:id/rules` | upsert des règles (1 ligne/compte) | `src/routes/accounts.rs` |
| `GET /trades/stats` | win rate, P&L, avg win/loss, profit factor, breakdown/symbole | `src/routes/stats.rs` |

Détails :
- Modèles ajoutés : `Account`, `NewAccount`, `PropRule`, `UpsertPropRule`,
  `TradeStats`, `SymbolStat` (`src/models.rs`).
- `/trades/stats` : agrégats SQL avec filtres optionnels (`account_id`, `symbol`,
  `from`, `to`). Win = `pnl > 0`, loss = `pnl < 0` ; break-even compté seulement
  dans `total_trades`. `profit_factor = null` s'il n'y a aucune perte.
- Route statique `/trades/stats` enregistrée avant `/trades/:id`.
- Tests d'intégration ajoutés : `accounts_create_list_and_rules`,
  `trade_stats_aggregates_correctly` (`tests/api.rs`).

## Reste à faire / prochaines étapes

- [ ] Compiler + faire passer `cargo test` sur la cible.
- [x] **Caler `mt5.rs` sur un export MT5 réel** (GoatFunded, 2026-06-28) : rapport
      multi-sections FR, parser réécrit (section-aware, bilingue FR/EN,
      auto-détection du délimiteur `,`/`;`, colonnes Heure/Prix en double).
      Validé : 13 positions extraites, total P&L 1789,01. Voir sprint-02 §import.
- [x] Endpoints `/accounts` et `/accounts/:id/rules` (Phase 2 — prop firm).
- [x] `GET /trades/stats` (analytics agrégées).
- [ ] Démarrer l'app React Native (login Keychain + liste trades).
- [ ] Mettre `Status: Accepted` sur l'ADR une fois le backend validé en prod.
