# Sprint 7 — Fix layout sélecteurs + multi-comptes

**Date:** 2026-06-29
**Objectif:** corriger le bug d'affichage des sélecteurs (Calendrier/News) et
ajouter la gestion multi-comptes (total + ventilation par compte).
**Statut:** code livré ; app à recharger (pas de changement backend).

---

## Fix layout (cases qui s'allongeaient)

Cause : un `ScrollView horizontal` dans un conteneur flex s'étire verticalement
et étire ses chips. Corrigé sur `AssetBar` et `FilterBar` (`flexGrow: 0`,
`alignItems: 'center'`), + `style={{flex:1}}` sur les `ScrollView` de contenu de
`CalendarScreen`/`NewsScreen` pour qu'ils occupent l'espace restant et scrollent.

## Multi-comptes

- API client : `createAccount` (POST /accounts existait déjà côté backend) + type `NewAccount`.
- `Filters` : ajout de `accountId` (null = tous = total) ; `filtersToQuery` →
  `account_id`. Les endpoints `/trades` et `/trades/stats` filtraient déjà par compte.
- **`AccountPicker`** (`components/AccountPicker.tsx`) : bouton + **modal** listant
  les comptes, « Tous les comptes » (mode total), et **création de compte** inline
  (nom + solde). Retourne `accountId | null`.
- **Dashboard** : picker au-dessus des filtres → total (Tous) ou un compte précis ;
  tout (cartes, score, courbe, calendrier P&L, breakdown) recalculé pour la sélection.
- **Prop Firm** : picker (un seul compte, sans « Tous ») → le tracker (drawdown,
  objectif, consistance) cible le compte choisi ; trades filtrés par `account_id`.

## Vérification

⚠️ Non lancé ici. Relecture statique. Aucun changement backend (POST /accounts et
les filtres existaient). À recharger côté app (`r`).

## Reste à faire / prochaines étapes

- [ ] Recharger l'app, vérifier le fix d'affichage + créer un 2e compte et basculer.
- [ ] Affecter l'import CSV/XLSX à un compte choisi (param `?account_id=`).
- [ ] Coach IA (Anthropic).
