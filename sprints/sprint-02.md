# Sprint 2 — App mobile (premier jet)

**Date:** 2026-06-20
**Objectif:** un premier jet de l'app React Native, **testable sur iPhone**
rapidement, branché sur le backend du Sprint 1 (login, liste trades, stats).
**Statut:** code livré, à lancer sur le Mac (voir `code/mobile/README.md`).

---

## Décision clé : Expo (managed) plutôt que React Native bare

L'objectif explicite était « voir comment tester sur mon iPhone ». Le chemin le
plus court est **Expo + l'app Expo Go** : on scanne un QR code, l'app se charge
sur le téléphone — **sans Xcode ni compte développeur Apple payant**.

Conséquences sur la stack vs `ARCHITECTURE.md` :
- `react-native-keychain` → remplacé par **`expo-secure-store`** (équivalent
  Keychain, compatible Expo Go). Même garantie : le JWT est stocké dans le
  trousseau sécurisé du device.
- Pas de `package.json` figé dans le repo : **Expo Go ne supporte que le dernier
  SDK**, donc on génère le projet avec le SDK courant (`create-expo-app@latest`)
  puis on dépose le code par-dessus (procédure dans `code/mobile/README.md`).
- Charts (`react-native-charts-wrapper`) repoussés : pas de natif custom dans ce
  jet, les stats sont rendues en cartes simples.

---

## Périmètre réalisé

| Élément | Détail | Fichiers |
|---------|--------|----------|
| Client API typé | `login`, `listTrades`, `getStats`, gestion d'erreurs (`ApiError`) | `src/api/client.ts`, `src/api/types.ts` |
| Auth | JWT + URL serveur stockés via `expo-secure-store`, restauration au lancement | `src/auth/AuthContext.tsx`, `src/hooks/useApi.ts` |
| Navigation | gate login/onglets, stack Journal (liste → détail), onglet Stats | `src/navigation/RootNavigator.tsx` |
| Écran Login | saisie **URL serveur** + mot de passe (pas d'édition de code pour tester) | `src/screens/LoginScreen.tsx` |
| Écran Journal | liste des trades, pull-to-refresh, P&L coloré, navigation détail | `src/screens/TradesScreen.tsx` |
| Détail trade | vue lecture seule de tous les champs | `src/screens/TradeDetailScreen.tsx` |
| Écran Stats | cartes (trades, win rate, profit factor, P&L, moy.) + breakdown par instrument | `src/screens/StatsScreen.tsx` |
| Thème | thème sombre partagé + helpers (`formatPnl`, `pnlColor`) | `src/theme.ts` |
| Doc test iPhone | prérequis, bootstrap, lancement, Tailscale, dépannage | `code/mobile/README.md` |

Détails UX :
- L'URL du serveur est saisie dans l'app et persistée — idéal pour tester sans
  recompiler (LAN aujourd'hui, Tailscale hors domicile).
- Sur `401`, l'app déconnecte automatiquement (token expiré/invalide).
- UI en français, commentaires de code en anglais (convention projet).

---

## Comment tester (résumé)

1. `node -v` OK + app **Expo Go** sur l'iPhone.
2. Bootstrap : `create-expo-app@latest` + `expo install` (voir README mobile).
3. Backend lancé sur le Mac (`cargo run`), récupérer son IP locale.
4. `npx expo start`, scanner le QR, saisir `http://<IP-Mac>:8080` + mot de passe.

⚠️ Comme pour le backend, le code mobile **n'a pas pu être lancé/compilé** dans
l'environnement de travail (réseau verrouillé, pas de npm/Expo). Il a été écrit et
relu ; la première exécution se fait sur ta machine.

---

## Incrément — saisie & édition de trade (2026-06-20, suite)

Ajout de la création et de l'édition de trade depuis l'app :

| Élément | Détail | Fichiers |
|---------|--------|----------|
| Client API | `createTrade` (POST), `updateTrade` (PUT) + types `NewTrade`/`UpdateTrade` | `src/api/client.ts`, `src/api/types.ts` |
| Formulaire | écran unique création/édition : symbole, direction (LONG/SHORT), prix entrée/sortie, lot, P&L, setup, émotion, notes | `src/screens/TradeFormScreen.tsx` |
| Navigation | bouton **+ Trade** sur le Journal, **Modifier** sur le détail, écran `TradeForm` dans la pile | `src/navigation/RootNavigator.tsx` |
| Refresh | la liste se rafraîchit **au focus** (`useFocusEffect`) après ajout/édition | `src/screens/TradesScreen.tsx` |

Détails :
- Champs numériques tolérants (virgule ou point, vide → `null`), validation du
  symbole requis, gestion d'erreur + déconnexion auto sur `401`.
- Après enregistrement, retour à la liste (`popToTop`) qui se recharge.
- Pas de suppression : le backend n'expose pas de `DELETE /trades` (hors périmètre).

## Incrément — parser MT5 calé sur export réel (2026-06-28)

Chris a fourni un vrai export depuis son compte funded GoatFunded
(`ReportHistory-XXXXXXXXX.xlsx`). Analyse et calibration du parser `mt5.rs` :

- Le rapport MT5 est **multi-sections** (Positions / Ordres / Transactions), pas
  un CSV à en-tête unique. En-têtes **en français** (`Heure, Position, Symbole,
  Type, Volume, Prix, S/L, T/P, Heure, Prix, Commission, Echange, Profit`),
  colonnes `Heure`/`Prix` en double (ouverture/clôture), symboles suffixés `.x`.
- Parser **réécrit** : détection de la section **Positions**, arrêt à la section
  suivante, aliases **bilingues FR/EN**, **auto-détection du délimiteur**
  (`,`/`;`/tab — Numbers en locale FR exporte en `;` + virgule décimale),
  normalisation des dates `2026.06.20 13:45:00` → ISO.
- **Validation** : prototype Python rejouant la logique exact sur le fichier réel
  → **13 positions** extraites, P&L total **1789,01**, fonctionne en `,` et en `;`.
  Tests unitaires Rust ajoutés avec un extrait réel + variantes (`mt5.rs`).
- Dédup par `mt5_ticket` = colonne `Position`.

Workflow d'export pour Mac : MT5 (app native macOS) → onglet Historique → Rapport
→ XLSX → enregistrer en CSV → import. Détaillé dans `backend/README.md`.

## Incrément — import XLSX bout-en-bout (2026-06-28)

Objectif : supprimer la corvée de conversion XLSX→CSV.

**Backend** (`code/backend`)
- Dépendance `calamine` ajoutée pour lire les `.xlsx`.
- `mt5.rs` refactoré : logique commune `parse_rows(rows, account_id)` partagée
  entre `parse` (CSV) et `parse_xlsx` (calamine → grille de cellules → `parse_rows`).
- `cell_to_string` formate les flottants entiers sans `.0` (ticket `42047402`).
- `import.rs` détecte le format par **magic bytes** (`PK\x03\x04` = xlsx) et route
  vers le bon parser. Le même endpoint accepte donc `.xlsx` **ou** `.csv`.

**App** (`code/mobile`)
- `expo-document-picker` ajouté.
- Client : `importFile(blob)` envoie les octets bruts en `application/octet-stream`.
- Écran **Journal** : bouton *« Importer un fichier MT5 (.xlsx / .csv) »* →
  sélecteur de fichier → upload → résumé en alerte → refresh de la liste.
- Le fichier exporté de MT5 s'importe **sans conversion**.

## Incrément — graphiques (2026-06-28)

Graphiques sur l'écran **Stats**, en **SVG pur** (`react-native-svg`, compatible
Expo Go — pas de lib native nécessitant un build custom) :

- `src/utils/series.ts` : calcul des séries depuis la liste des trades —
  `equitySeries` (P&L cumulé chronologique) et `dailyPnlSeries` (somme par jour).
- `src/components/Charts.tsx` : `EquityCurve` (ligne + aire, ligne de zéro) et
  `PnlBars` (barres vertes/rouges centrées sur zéro), largeur responsive.
- `StatsScreen` : récupère stats **et** trades (`Promise.all`), calcule les séries
  via `useMemo`, affiche les deux graphiques au-dessus du breakdown par instrument.
- Dépendance ajoutée : `react-native-svg`.

## Reste à faire / prochaines étapes

- [ ] Lancer l'app sur l'iPhone et valider login → import XLSX → liste → stats+graphiques.
- [x] Bouton **import depuis l'app**.
- [x] Accepter directement le **XLSX** côté backend.
- [x] **Graphiques** (equity curve, P&L par jour).
- [ ] Face ID (expo-local-authentication) pour déverrouiller.
- [ ] Filtres sur les stats (compte / période / instrument).
- [x] Saisie / édition de trade (`POST` / `PUT /trades`) + tags & notes.
- [ ] Import CSV depuis l'app (sélecteur de fichier → `POST /trades/import/csv`).
- [ ] Face ID (expo-local-authentication) pour déverrouiller.
- [ ] Graphiques (equity curve, P&L par jour).
