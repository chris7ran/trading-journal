# Sprint 14 — Config prop firm sur le Dashboard + fix clavier

**Date:** 2026-06-29
**Objectif:** loger la config prop firm là où elle a du sens (avec le compte, sur
le Dashboard), et corriger les champs de saisie masqués par le clavier.
**Statut:** livré, **100% mobile** (aucun backend → recharger l'app).

---

## Config prop firm → carte Dashboard (view + edit)

Retour utilisateur : la config n'a pas sa place dans Setups ; elle appartient au
**compte** (drawdown/objectif/**consistance**), visible sur le Dashboard.

- `components/PropFirmCard.tsx` : la carte affiche les jauges (objectif, drawdown,
  consistance) **et** un bouton **⚙ / « Configurer les règles »** ouvrant un
  **modal** : solde de départ, objectif %, DD global %, DD journalier %,
  **consistance %**. Enregistrement via `updateAccount` + `upsertRules`.
- `screens/SetupsScreen.tsx` : section prop firm **retirée** → Setups redevient la
  gestion pure des setups (liste + création).

## Fix clavier (toute l'app)

Les champs bas de page passaient sous le clavier. Corrigé :
- `SetupsScreen` : passé en `KeyboardAvoidingView` + `ScrollView`
  `automaticallyAdjustKeyboardInsets` + `keyboardShouldPersistTaps`.
- `TradeFormScreen` : ajout `automaticallyAdjustKeyboardInsets` (déjà en KAV).
- `AccountPicker` (modal) et le nouveau **modal prop firm** : enveloppés dans un
  `KeyboardAvoidingView` (le bottom-sheet remonte au-dessus du clavier).
- Login utilisait déjà un `KeyboardAvoidingView`.

## Vérification

⚠️ Non lancé ici. Relecture statique. Mobile only → `r`. Réutilise les endpoints
existants (`/accounts`, `/accounts/:id/rules`).

## Reste à faire / prochaines étapes

- [ ] Recharger : Dashboard → sélectionner un compte prop firm → « Configurer »,
      saisir les règles ; vérifier que plus aucun champ ne passe sous le clavier.
- [ ] (Option) marquer un compte comme « funded » explicitement (flag is_funded).
