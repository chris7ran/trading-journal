# Sprint 21 — Fiche macro : mois dernier + forecast + scénarios

**Date:** 2026-07-05
**Objectif:** dans la fiche détail d'un indicateur macro, ajouter le mois dernier
et le forecast (Forex Factory) au-dessus de l'historique annuel (World Bank), plus
un commentaire structuré : constat, scénarios, scénario le plus probable.
**Statut:** livré, **100% mobile** → recharger (`r`). Implémenté avec le modèle Fable.

---

## Changements

- `utils/macro.ts` :
  - `parseMetric(s)` : extrait le nombre d'une chaîne FF ("2.9%", "7.28M", "3.86|2.9").
  - `regionCurrency(region)` : région → devise.
  - `macroCommentary(ind, event)` : renvoie `{ constat, scenarios[], baseCase }`.
    - **constat** : tendance annuelle (WB) + ce que le consensus attend
      (forecast vs mois dernier).
    - **scénarios** : réaction attendue si le chiffre sort au-dessus / conforme /
      sous l'estimé, avec la logique hawkish/dovish par type d'indicateur
      (inflation, chômage, PIB) et l'implication sur la devise.
    - **baseCase** : le scénario le plus probable = le consensus (forecast), déjà
      intégré par le marché ; l'impact vient de l'écart au chiffre publié.
- `components/MacroDetail.tsx` :
  - bloc « Prochaine publication » : **Estimé** + **Mois dernier** (forecast/previous FF).
  - section **Constat**, section **Scénarios** (3 cas), encadré **Scénario le plus
    probable**. Repli sur `sentimentText` si aucune publication correspondante.
  - **Historique annuel** (World Bank) conservé en bas.

## Note sur les données

- Mois dernier + estimé = calendrier **Forex Factory** (mensuel, précis).
- Historique + variation annuelle = **World Bank** (annuel).
- Le « scénario le plus probable » reflète le **consensus** (forecast) — ce n'est
  pas une prédiction du chiffre, mais la base déjà anticipée par le marché.

## Vérification

Relecture statique (auto + relecture du diff). Mobile only → `r`.
