# Sprint 5 — Consistance prop firm, calendrier éco & news

**Date:** 2026-06-29
**Objectif:** règle de consistance GoatFunded (20%), calendrier économique
rouge/orange, et fil de news fondamentales.
**Statut:** code livré ; backend à recompiler (nouvelles dépendances réseau).

---

## Règle de consistance (GoatFunded 20%)

Recherche : aucun jour ne doit représenter ≥ 20% du profit total pour demander un
payout. `consistance % = meilleur jour P&L / P&L total`. → total requis ≥ 5× le
meilleur jour. Ne ferme pas le compte, bloque le retrait.
([source GoatFunded](https://help.goatfundedtrader.com/en/articles/15290379-what-is-the-consistency-rule))

Impl (`utils/propfirm.ts`, `screens/PropFirmScreen.tsx`) : meilleur jour, ratio de
consistance vs limite (`consistency_rule_pct`, défaut 0.20), barre dédiée + alerte
+ montant total requis pour débloquer le payout.

## Calendrier éco + news (Macro)

**Backend** (`src/routes/market.rs`, dép. `reqwest` rustls + `feed-rs`)
- `GET /macro/calendar` : flux JSON gratuit **faireconomy** (mirror Forex Factory,
  `ff_calendar_thisweek.json`), filtré High→`red` / Medium→`orange`.
- `GET /macro/news` : agrège plusieurs flux **RSS** (FXStreet, Investing, Nasdaq)
  via `feed-rs` ; tolérant (saute les flux injoignables), trié par date, top 40.
- Modèles `EcoEvent`, `NewsItem`. Routes protégées (JWT).

**Mobile** (`screens/MacroScreen.tsx`, 5e onglet « Macro »)
- Calendrier : pastille rouge/orange, devise, heure, prévision/précédent.
- News : source + titre cliquable (`Linking.openURL`). `Promise.allSettled` pour
  qu'une source en panne ne vide pas l'écran.
- Client : `getCalendar`, `getNews` + types `EcoEvent`/`NewsItem`.

## Vérification

⚠️ Non compilé/lancé ici. Consistance = calcul pur (fiable). Calendrier/news font
des appels réseau **au runtime depuis ton serveur** — à valider après recompile :
- `reqwest`/`feed-rs` ajoutent du temps de compilation (TLS rustls).
- Certains flux RSS peuvent bloquer ou changer ; le code est tolérant (saute).
- faireconomy = source communautaire ; si l'URL change, adapter `CALENDAR_URL`.

## Correctif — rate-limit faireconomy → cache mémoire (2026-06-29)

Au test : `nslookup` OK (Cloudflare 104.18.x), mais `curl` renvoyait la page
**« Rate Limited »** (faireconomy : 2 téléchargements / 5 min). L'app interrogeant
le calendrier à chaque focus, on dépassait la limite → HTML au lieu du JSON.

Fix (`market.rs`) : **cache mémoire** (`static Mutex<Option<(Instant, Vec<…>)>>`),
TTL 15 min calendrier / 10 min news. On ne tape les sources qu'après expiration ;
en cas d'échec/rate-limit on **sert le cache périmé** plutôt qu'une erreur. Modèles
`EcoEvent`/`NewsItem` passés en `Clone`. (Le faux fallback `cdn-nfs` — inexistant,
d'où l'erreur DNS — a été retiré.)

## Reste à faire / prochaines étapes

- [x] Cache calendar/news (en mémoire ; persistance en base = amélioration future).
- [ ] Recompiler le backend, valider `/macro/calendar` et `/macro/news`.
- [ ] Filtrer le calendrier par devises suivies (USD/EUR) dans l'app.
- [ ] Sources fondamentales dédiées DAX40 / indices US (au-delà du RSS généraliste).
