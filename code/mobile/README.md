# Trading Journal — App mobile (React Native / Expo)

App iOS/iPad, design sombre violet « web 3.0 » (style TradeZella), branchée sur le
backend Rust. Testable sur iPhone via **Expo Go**.

> Ce dossier est un projet Expo (généré par `create-expo-app`) dans lequel sont
> déposés `App.tsx` et tout le code sous `src/`.

## Fonctionnalités

- ✅ Login JWT (bouton dégradé), token dans le Keychain (`expo-secure-store`).
- ✅ **Face ID / Touch ID** : verrou biométrique au lancement (si dispo).
- ✅ **Multi-comptes** : sélecteur (dropdown) « Tous les comptes » (total) ou un
  compte précis, avec création de compte ; sur Dashboard et Prop Firm.
- ✅ **Dashboard** (unifié) : en-tête dégradé (Net P&L), jauge de score, cartes,
  courbe d'equity, P&L/jour, **calendrier P&L** et **breakdown par instrument**.
  Filtres période (30j/90j/YTD) + instrument + compte.
- ✅ **Journal** : liste (pills LONG/SHORT), détail, ajout/édition ; bouton **« + »**
  (import MT5 ou saisie manuelle) ; **calendrier P&L** cliquable (tap → trade).
- ✅ **Setups** (remplace Prop Firm) : patterns/stratégies + règles, assignés aux trades.
- ✅ **Revue de trade** : choix du setup + questions (plan suivi, SL respecté, pattern
  valide, thèse validée, sortie maîtrisée) sur chaque trade.
- ✅ **Prop Firm** : objectif de profit, drawdown global & journalier, **règle de
  consistance (20% GoatFunded)**, alertes, config du solde et des règles.
- ✅ **Calendrier éco** : événements rouge/orange, sélecteur d'actif (USD/EUR/US30/DAX40).
- ✅ **Économie** : indicateurs macro (US / zone euro / Allemagne) via World Bank —
  cartes avec **mini-courbes (sparklines)** + le calendrier éco, dans un seul onglet « Éco ».
- ✅ **News** : fil agrégé (RSS), cliquable, sélecteur d'actif, **sentiment** par
  article (haussier/baissier/neutre) + **jauge d'humeur du marché**.
- ✅ **Coach** : insights à base de règles (win rate par instrument, ratio gain/perte,
  revenge-trading, consistance, série de pertes, heure, émotions) — gratuit, calculé
  sur l'appareil, par compte ou total.

## Lancer (sur le Mac)

Prérequis : Node.js LTS, et l'app **Expo Go** sur l'iPhone.

```bash
cd code/mobile
npx expo start          # affiche un QR code
```
Sur l'iPhone (même Wi-Fi que le Mac), scanne le QR avec Expo Go. Dans l'app :
URL serveur = `http://<IP-du-Mac>:8080` (le backend `cargo run`), + ton mot de passe.

### (Ré)installation des dépendances

Si tu repars d'un projet Expo neuf, réinstalle les libs (versions alignées au SDK) :
```bash
npx expo install @react-navigation/native @react-navigation/native-stack \
  @react-navigation/bottom-tabs react-native-screens \
  react-native-safe-area-context expo-secure-store expo-document-picker \
  react-native-svg expo-linear-gradient expo-local-authentication
```
`@expo/vector-icons` est déjà inclus dans le template Expo. Les filtres et le
tracker prop firm n'ajoutent aucune dépendance native.

## Arborescence (src/)

```
src/
├── api/         client HTTP typé + types
├── auth/        AuthContext (JWT + verrou Face ID)
├── components/  Charts, ScoreGauge, CalendarHeatmap, FilterBar
├── hooks/       useApi()
├── navigation/  RootNavigator (login → lock → onglets)
├── screens/     Login, Lock, Dashboard, Trades, TradeDetail, TradeForm, Stats, PropFirm
└── utils/       series, score, filters, propfirm
```

## Dépannage

- « Réseau injoignable » → mauvaise URL/IP serveur, backend éteint, ou pare-feu (port 8080).
- Écran blanc / version Expo Go → régénère avec `create-expo-app@latest` (SDK courant).
- 401 → mot de passe ≠ hash du `.env` backend.
- iPhone ne voit pas le QR → même réseau requis, sinon `npx expo start --tunnel`.
