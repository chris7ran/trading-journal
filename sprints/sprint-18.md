# Sprint 18 — En-tête unifié + menu burger (profil, guide, CGU)

**Date:** 2026-07-05
**Objectif:** harmoniser l'en-tête de tous les écrans (fin de la capsule iOS
incohérente) et remplacer le « Déconnexion » par un vrai menu burger.
**Statut:** livré, **100% mobile** → recharger (`r`).

---

## Pourquoi l'incohérence ?

Journal et Setups sont des *stacks* → iOS leur dessine une barre **native**, avec
la capsule grise automatique (iOS 26 « Liquid Glass ») autour des boutons. Les
autres onglets utilisent un en-tête JS → pas de capsule. D'où le burger tantôt
entouré, tantôt non.

## Solution — en-tête custom identique

- `components/AppHeader.tsx` (`HeaderBar`) : en-tête JS unique (titre centré +
  burger rond dessiné par nous), monté via l'option `header` de la navigation sur
  **les 6 écrans** (Dashboard, Journal, Setups, Économie, News, Coach). Plus de
  barre native en haut de ces écrans → rendu **identique** partout, capsule iOS
  supprimée. Gère le safe-area (encoche).

## Menu burger

Bottom-sheet avec : **Mon profil**, **Changer de compte**, **Se connecter**
(placeholder → profil pour l'instant, à câbler), **Guide utilisateur**,
**Conditions d'utilisation**, **Déconnexion**.

- `navigation/RootNavigator.tsx` : nouveau stack racine `AuthedApp` = `Tabs` +
  écrans `Profile` / `Guide` / `Terms` (accessibles depuis n'importe quel écran via
  le burger, la navigation « remonte » au stack racine). Les écrans du menu et les
  écrans de détail (trade, setup) gardent la barre native avec bouton retour.

## Nouveaux écrans

- `screens/ProfileScreen.tsx` : statut, serveur, version, note sur la gestion des
  comptes de trading + bouton Déconnexion.
- `screens/GuideScreen.tsx` : guide utilisateur rédigé (Dashboard, Journal, Import
  MT5, Setups, Économie, News, Coach, revue post-trade, prop firm).
- `screens/TermsScreen.tsx` : CGU de base (usage perso, pas de conseil financier,
  données auto-hébergées, sources tierces, responsabilité).

## Vérification

⚠️ Non lancé ici. Relecture statique. Mobile only → `r`.

## Reste à faire

- [ ] Câbler « Se connecter / Changer de compte » (multi-utilisateur) plus tard.
- [ ] Laisser Chris éditer le texte du guide / des CGU si besoin.
