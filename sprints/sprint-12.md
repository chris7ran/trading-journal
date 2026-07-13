# Sprint 12 — Analytics de revue : top mistakes + performance par setup (étape 3)

**Date:** 2026-06-29
**Objectif:** exploiter les revues de trade (sprint 11) en analytics façon AccuTrader.
**Statut:** livré, **100% mobile** (aucun backend → recharger l'app).

---

## Fait

`utils/analytics.ts`
- `topMistakes(trades)` : pour chacune des 5 dimensions de revue, compte combien de
  fois la règle a été **enfreinte** (réponse « Non »), sur combien de trades notés,
  et l'**impact P&L** cumulé. Trié par fréquence.
- `setupPerformance(trades)` : perf par setup (`setup_tag`) — nombre, win rate, P&L
  (bucket « Sans setup » inclus). Trié par P&L.

`screens/CoachScreen.tsx`
- Section **« Top erreurs à éviter »** : chaque règle enfreinte avec le compte
  (x/notés) et l'impact P&L coloré. Message d'incitation si aucune revue.

`screens/SetupsScreen.tsx`
- Section **« Performance par setup »** : chaque setup avec nombre, win rate, P&L.
  (Récupère aussi les trades pour le calcul.)

## Vérification

⚠️ Non lancé ici. Relecture statique. Mobile only → `r`. Les analytics n'apparaissent
qu'une fois quelques trades **notés** (setup + questions de revue).

## Reste à faire / prochaines étapes

- [ ] Recharger, noter quelques trades, vérifier Coach (top mistakes) et Setups (perf).
- [ ] Filtrer top mistakes / perf setup par compte & période (comme le Dashboard).
- [ ] Reloger la config des règles prop firm.
