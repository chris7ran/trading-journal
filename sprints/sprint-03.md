# Sprint 3 — Refonte UI/UX « web 3.0 » (style TradeZella)

**Date:** 2026-06-28
**Objectif:** refondre l'app dans le langage visuel de TradeZella — fond sombre
violacé, dégradés violet→bleu, jauge de score, calendrier P&L, Dashboard.
**Statut:** code livré, à lancer sur le Mac (voir `code/mobile/README.md`).

---

## Analyse de référence (tradezella.com)

Signature retenue : fond très sombre légèrement violet, **accent violet/indigo**
avec **dégradés violet→bleu**, P&L vert/rouge, cartes arrondies, et composants
iconiques — **calendrier P&L mensuel**, **score circulaire** (Zella Score),
courbe d'equity, cartes de stats.

## Réalisé

**Thème** (`src/theme.ts`)
- Nouvelle palette : `bg #0B0B12`, `card #15151F`, `primary #7C5CFC`,
  `accentBlue #5B8DEF`, `green #16C784`, `red #F6465D`. Tokens `gradients.brand`,
  `radius`, helper `formatPnlCompact`.

**Composants**
- `ScoreGauge` (`components/ScoreGauge.tsx`) : jauge circulaire à dégradé (SVG).
- `CalendarHeatmap` (`components/CalendarHeatmap.tsx`) : calendrier mensuel P&L,
  mois déduit des données, cases vertes/rouges + total mensuel.
- `Charts` : courbe d'equity et aire passées en **dégradé** violet→bleu.
- `utils/score.ts` : score composite 0–100 (win rate + profit factor + profitabilité).

**Écran Dashboard** (`screens/DashboardScreen.tsx`, nouveau)
- En-tête en **dégradé** (`expo-linear-gradient`) avec Net P&L, win%, PF.
- Carte score (jauge + label qualitatif), 4 cartes métriques, courbe d'equity,
  histogramme P&L/jour. Récupère stats + trades en parallèle, refresh au focus.

**Restyle + navigation**
- Onglets : **Dashboard / Journal / Stats** avec icônes (`@expo/vector-icons`).
- Login : bouton en dégradé.
- Journal : pills LONG/SHORT colorées sur les lignes.
- Stats : ajout du **calendrier P&L** au-dessus des graphiques.

**Dépendances ajoutées** : `expo-linear-gradient` (et `@expo/vector-icons`, déjà
inclus dans le template Expo).

## Vérification

⚠️ Non compilé/lancé ici (réseau verrouillé). Code écrit + relu (relecture
statique). Première exécution sur ta machine via Expo Go.

## Incrément — navigation calendrier + Face ID (2026-06-28)

- **Calendrier** : navigation mois précédent/suivant (chevrons ‹ ›). Le calendrier
  suit le dernier mois des données tant que l'utilisateur n'a pas navigué
  manuellement (`useRef` + `useEffect`). Affiche « Aucun trade » pour un mois vide.
- **Face ID / Touch ID** (`expo-local-authentication`) : au lancement, si une
  session est persistée et que l'appareil a la biométrie configurée, l'app est
  **verrouillée** ; `LockScreen` propose le déverrouillage (prompt auto) + une
  sortie « Se déconnecter ». `AuthContext` gère `locked`/`unlock`, et
  `RootNavigator` ajoute le gate (loading → login → lock → tabs).

## Incrément — fix import XLSX réel (encodage UTF-16) (2026-06-29)

Au premier test sur la machine de Chris, l'import direct du `.xlsx` MT5 échouait :
`calamine` renvoyait `Unexpected end of xml, expecting '</sst>'`.

Cause racine identifiée : **MT5 écrit les XML internes du `.xlsx` en UTF-16**
(BOM `FF FE`), que `calamine`/quick-xml ne sait pas lire. Le fichier est pourtant
valide (lu sans souci par openpyxl).

Correctif (`src/csv_import/mt5.rs`, dép. `zip`) :
- `normalize_xlsx_encoding()` : ouvre l'archive `.xlsx`, transcode chaque entrée
  XML UTF-16 → UTF-8 (`transcode_utf16`, gère BOM LE/BE + corrige la déclaration
  `encoding=`), et reconstruit un zip propre, avant de le passer à `calamine`.
- Les entrées non-UTF-16 et les dossiers sont recopiés tels quels.

Contournement utilisé en attendant le recompile : conversion XLSX→CSV côté Mac
(le parser CSV était déjà validé) — import OK, 13 trades, +1789.

## Reste à faire / prochaines étapes

- [ ] Lancer sur iPhone et valider le rendu (dégradés, jauge, calendrier, Face ID).
- [x] Calendrier : navigation mois précédent/suivant.
- [x] Face ID (expo-local-authentication).
- [ ] Filtres (compte / période / instrument) sur Dashboard et Stats.
- [ ] Verrouiller aussi au retour en avant-plan (AppState background → lock).
