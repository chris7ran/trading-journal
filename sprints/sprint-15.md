# Sprint 15 — Effet glass + fix bouton « Add New »

**Date:** 2026-07-04
**Objectif:** retouches UI — recentrer le bouton « Add New » du Journal et donner un
look glass (verre dépoli) moderne aux cartes/tableaux et au menu du bas.
**Statut:** livré, **100% mobile** — ⚠️ nécessite `npx expo install expo-blur`.

---

## Bouton « Add New » (header Journal)

Le pill vert était décalé (padding vertical + gros `marginRight`). Corrigé :
hauteur fixe `30`, `justifyContent: 'center'`, `borderRadius: 15`,
`marginRight: spacing.sm`. Le header centre alors verticalement le bouton et
l'icône `+` + le texte sont alignés.

## Effet glass (verre dépoli)

- `theme.ts` : les surfaces `card` / `card2` / `border` passent en **blancs
  translucides** (`rgba(255,255,255,0.05 / 0.09 / 0.12)`) au lieu de gris opaques.
  Sur le fond sombre, ça donne des cartes/tableaux frostés avec un liseré
  lumineux. Ajout d'une couleur `surface` **opaque** (`#15151F`) pour ce qui doit
  rester net.
- `navigation/RootNavigator.tsx` : le **menu du bas** utilise un `BlurView`
  (`expo-blur`, tint dark, intensity 30) en `tabBarBackground`, avec
  `tabBarStyle` transparent + hairline. Vrai flou dépoli. La barre reste dans le
  flux (non absolue) → **aucun contenu masqué**, pas de padding à ajouter.
- Les **en-têtes** et les **bottom-sheets** (AccountPicker, config prop firm)
  pointent vers `colors.surface` (opaque) pour rester lisibles au-dessus du
  backdrop assombri.

## Dépendance

`expo-blur` n'était pas installé. À faire côté client :

```
cd code/mobile
npx expo install expo-blur
npx expo start -c        # redémarrage Metro requis (pas juste « r »)
```

## Vérification

⚠️ Non lancé ici. Relecture statique. Nouvelle lib native (`expo-blur`) → un
simple `r` ne suffit pas, il faut réinstaller + redémarrer Metro.

## Reste à faire / prochaines étapes

- [ ] `npx expo install expo-blur` puis `npx expo start -c`.
- [ ] Vérifier : bouton « Add New » centré ; cartes/tableaux frostés lisibles ;
      menu du bas en verre dépoli ; modals (compte / prop firm) toujours nets.
