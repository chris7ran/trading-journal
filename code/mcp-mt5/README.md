# Pont MCP MetaTrader 5

Expose un ou plusieurs terminaux MetaTrader 5 comme un serveur MCP, pour
consulter comptes, positions, historique et spécifications d'instruments
depuis un assistant.

**L'exécution est éteinte par défaut, compte par compte.** Tant que
`execution.enabled` est faux, le processus qui pilote ce terminal n'importe
même pas le module d'envoi : la capacité n'existe pas, au lieu d'exister et
d'être refusée. C'est la différence entre une porte fermée à clé et un mur.

Les appels MT5 qui écrivent vivent tous dans `orders.py`, et nulle part
ailleurs — chercher `order_send` dans les sources ne doit ramener que ce
fichier et ses tests.

## Pas de trace, pas d'ordre

Si l'écriture du journal d'audit échoue, l'envoi n'a pas lieu. C'est ce qui
sépare un audit d'un fichier de logs : un log perdu fait un trou dans
l'histoire, un audit perdu fait un ordre dont personne ne peut dire d'où il
vient.

Deux écritures par envoi — l'intention avant l'appel au broker, le résultat
après. Une **intention sans résultat** est le signal le plus précieux du
fichier : elle dit qu'un ordre est parti sans qu'on sache ce qu'il est devenu.
`Audit.orphans()` les retrouve, et c'est la première question à poser après un
incident.

Le journal écrit aussi une ligne au démarrage, pour découvrir qu'un disque est
plein ou qu'un chemin est faux avant qu'un ordre n'en dépende.

## Pourquoi un processus par terminal

Le paquet Python `MetaTrader5` ne gère **qu'une connexion de terminal par
processus** : appeler `initialize()` avec un second chemin ferme la première.
Servir plusieurs comptes depuis un seul processus imposerait donc de se
reconnecter entre chaque appel — lent, et on y perd l'état du terminal.

Le processus que vous démarrez ne détient donc aucune connexion MT5. Il lance
un enfant par terminal configuré (`worker.py`) et leur route les appels par
des tubes. Un enfant qui meurt est relancé au prochain appel.

## Prérequis

- Windows, avec un terminal MT5 par compte, installé **en mode portable**
- Python 3.11 ou plus récent (`tomllib` est utilisé pour la configuration)
- Chaque terminal lancé au moins une fois avec `/portable`, et connecté

## Installation

```powershell
cd C:\...\code\mcp-mt5
py -3.12 -m pip install -r requirements.txt
copy accounts.example.toml accounts.toml
notepad accounts.toml
```

`accounts.toml` ne doit jamais être commité : il contient des numéros de
comptes réels et des chemins de machine. Le `.gitignore` du dépôt l'exclut
déjà.

## Vérification

Avant de démarrer le serveur, lancez le contrôle hors réseau :

```powershell
py -3.12 selftest.py
```

Il démarre chaque terminal, affiche solde, levier, mode de marge, décalage
d'horloge, et relève les spécifications de tous les symboles déclarés. Il
isole les problèmes de configuration des problèmes de protocole.

Trois lignes méritent attention dans sa sortie :

- **`mode marge`** doit valoir `hedging`. En `netting`, un second ordre sur le
  même symbole fusionne avec la position existante au lieu de se placer à
  côté, ce qui invalide toute gestion par jambes.
- **`algo trading`** à `False` signifie que le bouton *Algo Trading* du
  terminal est éteint. Sans conséquence tant qu'on lit, bloquant le jour où
  l'on enverra un ordre.
- **`horloge`** : hors séance l'écart n'est pas mesurable, et les horodatages
  sortent alors en heure serveur. Voir plus bas.

## Démarrage

```powershell
$env:MCP_MT5_TOKEN = [guid]::NewGuid().ToString("N")
py -3.12 server.py
```

Le serveur n'écoute que sur `127.0.0.1:8765` et refuse toute autre interface.
Il est publié sur le tailnet par Tailscale, ce qui n'ouvre aucun port sur la
box :

```powershell
tailscale serve --bg --https=8443 http://127.0.0.1:8765
```

Côté client, l'URL est `https://<machine>.<tailnet>.ts.net:8443/mcp`, avec
l'en-tête `Authorization: Bearer <MCP_MT5_TOKEN>`.

### Déclarer le nom public

Derrière un proxy, il faut annoncer le nom sous lequel le serveur est joint,
sans quoi la protection anti-DNS-rebinding répond `421 Invalid Host header` :
Tailscale transmet l'en-tête `Host` d'origine, que le serveur ne reconnaît
pas.

```powershell
$env:MCP_MT5_ALLOWED_HOSTS = "mamachine.montailnet.ts.net:8443"
py -3.12 server.py
```

ou, de façon équivalente, `--allowed-host mamachine.montailnet.ts.net:8443`,
répétable. Le nom passe par l'environnement ou la ligne de commande et jamais
par un fichier suivi : un nom de machine et un tailnet n'ont rien à faire dans
un dépôt public.

Désactiver la protection serait la correction paresseuse et une mauvaise idée :
c'est elle qui empêche une page web visitée par hasard de viser ce serveur à
travers le navigateur de la machine.

Le jeton est une défense de second rideau, pas le verrou principal : c'est
l'authentification des machines du tailnet qui filtre réellement les accès. Il
couvre le cas que cette couche ne peut pas couvrir — un autre processus tournant
sur la même VM Windows.

## Les outils

| Outil | Ce qu'il répond |
|---|---|
| `list_accounts` | Comptes configurés et leur état. N'ouvre aucun terminal. |
| `get_account` | Solde, équité, marge, levier, mode de couverture, base de temps |
| `get_positions` | Positions ouvertes, filtrables par symbole |
| `get_pending_orders` | Ordres limites et stops non déclenchés |
| `get_history` | Trades clôturés sur une période, un objet par position |
| `get_symbol_spec` | Pas de volume, valeur du tick, modes d'exécution, distance minimale |
| `get_quote` | Bid, ask, spread en points |
| `list_market_watch` | Symboles présents dans le Market Watch |
| `get_clock` | Décalage entre l'horloge du broker et l'UTC réel |
| `preview_plan` | Chiffre un plan de trade et le passe aux garde-fous, **sans rien envoyer** |
| `execute_plan` | Envoie le plan au marché, une jambe par cible |
| `schedule_plan` | Dépose le plan en attente au niveau voulu, avec expiration |
| `cancel_plan` | Retire les ordres en attente d'un plan, par son `magic` |

Tous prennent un `account` : l'alias défini dans `accounts.toml`. Les trois
derniers ne répondent que si le compte a activé l'exécution.

## Envoyer

**Tout est recalculé au moment de l'envoi.** Une prévisualisation d'il y a
cinq minutes ne vaut rien sur un marché qui bouge — ce matin, l'or a fait
passer le même plan de 0,92 à 0,73 R en dix minutes. `execute_plan` refait
donc le dimensionnement, les garde-fous et le verdict, et exécute ce qu'il
vient de juger plutôt qu'un chiffrage reçu en argument.

Un plan en zone orange exige `confirm=true`. Un plan refusé par un garde-fou
l'est quoi qu'il arrive : **`confirm` lève une hésitation, pas un interdit.**

### Échec à mi-chemin

Les jambes partent une par une, et l'envoi s'arrête à la première refusée.
Rien n'est défait automatiquement : chaque jambe déjà passée porte son propre
stop, donc elle est protégée, et tenter de la refermer dans la foulée
ajouterait un envoi qui peut lui aussi échouer — on remplacerait un plan
incomplet par une cascade. Ce qui est garanti, c'est que la réponse dit
exactement quelles jambes sont ouvertes et laquelle a été refusée.

### Attendre plutôt qu'entrer dégradé

`schedule_plan` dépose les jambes au niveau prévu et laisse le prix revenir.
Le dimensionnement part de ce niveau, puisque c'est là que l'ordre se
remplira.

Le plan **occupe un créneau et réserve son risque dès la pose**, pas au
déclenchement. Sinon un ordre posé le matin, rempli l'après-midi après deux
pertes, aurait été validé contre un état périmé.

Quand le compte déclare sa distribution d'issues, `preview_plan` ajoute
`wait_instead` : la probabilité minimale de retour du prix au-delà de laquelle
attendre rapporte davantage qu'entrer maintenant. Un seuil, pas une
recommandation — la mesure sur l'historique vient le confronter.

## Chiffrer un plan

`preview_plan` prend une idée — symbole, sens, stop, cibles, risque voulu — et
rend la taille de position, le découpage en jambes et le verdict des
garde-fous. **Il n'envoie rien** : aucune fonction de ce module ne sait placer
un ordre.

### Le dimensionnement part du prix réel

Pas du prix écrit dans l'idée. Le stop, lui, n'a pas bougé : entrer dix points
plus haut rend la perte réelle dix points plus grande que celle budgétée.
`entry` et `entry_low`/`entry_high` servent uniquement à mesurer ce qu'un
retard a coûté.

```
volume = (équité × risque%) / (distance_au_stop × valeur_du_point)
```

arrondi **vers le bas** au pas du symbole, jamais vers le haut : dépasser
silencieusement le risque demandé serait pire que ne rien garantir.

### Une jambe par cible

Plusieurs positions ouvertes d'un coup, pas une position découpée ensuite.
Chaque jambe porte sa propre cible, déposée chez le broker, donc les sorties
partent même si cette machine redémarre en pleine séance. Le reste de la
division tombe sur la cible la plus lointaine, celle qui a le plus de chances
d'être laissée courir.

### Le ratio qui décide

Le **R moyen pondéré par le volume**, pas celui de la cible la plus lointaine :
c'est l'espérance du plan tel qu'il sera exécuté. Trois zones, réglables par
compte dans `[account.limits]` — au-dessus de `r_green` on exécute, entre les
deux on alerte et l'humain tranche, sous `r_red` on refuse sans dérogation.

Un cas mérite attention : quand le prix est passé de l'autre côté de l'entrée,
**vers le stop**, le ratio s'améliore mécaniquement puisque le stop est plus
proche. Le plan le signale, parce qu'un chiffre flatteur est le plus dangereux
au moment précis où le scénario se dégrade.

### Deux niveaux d'avertissement

Un avertissement qui se déclenche sur tous les plans finit par ne plus être
lu, et une confirmation systématiquement requise finit cliquée sans réflexion
— c'est ainsi qu'on perd le bénéfice d'avoir demandé.

Ne demandent un **oui explicite** que les remarques disant quelque chose que
le ratio ne dit pas : le prix passé de l'autre côté de l'entrée, et le volume
plafonné par le broker, qui signifie que le risque réel n'est plus celui
demandé.

La simple **dérive de prix** reste informative. Le prix bouge toujours entre
la lecture d'une idée et son chiffrage, et sa conséquence *est* la perte de
ratio — déjà mesurée, déjà classée en zones. Si elle compte vraiment, la zone
passe orange d'elle-même et c'est elle qui réclame un humain.

### Deux couches de garde-fous

Celles du pont, dans `accounts.toml`, s'appliquent **toujours** : risque par
trade, perte journalière, plans simultanés, marge. Elles existent pour les
comptes sans prop firm derrière — une démo n'a aucune règle et doit rester
impossible à faire sauter.

Celles de la firme, lues depuis le journal, ne s'appliquent qu'aux comptes qui
en déclarent. Le pont coupe avant qu'elles ne soient atteintes plutôt qu'au
moment où elles sont enfreintes.

Le garde-fou le plus utile est celui qui refuse un plan dont la **perte
complète** dépasserait l'enveloppe du jour, au lieu de le découvrir une fois
le stop touché. Un plafond vérifié seulement après coup est un rapport, pas un
plafond.

Le verdict vaut `accepté`, `à confirmer` ou `refusé`, et liste **tous** les
motifs : corriger un refus pour tomber sur le suivant n'apprend rien sur
l'endroit où est vraiment le mur.

## Le watcher

Dès qu'une jambe sort **en gain**, les jambes restantes du même plan voient
leur stop remonter au prix d'entrée. Rien ne bouge avant, et rien ne bouge si
la seule sortie a été une perte — le stop a alors fait son travail et il n'y a
plus rien à protéger.

Il démarre avec le serveur, un par compte qui exécute, et partage les mêmes
poignées de terminal : le verrou par compte sérialise ses appels avec ceux des
outils.

**Ce n'est pas un point de défaillance critique.** Les cibles et les stops
initiaux vivent chez le broker. Si ce processus meurt, les sorties partent
quand même ; ce qu'on perd est seulement l'amélioration du stop après une
première sortie. C'est la raison pour laquelle il peut vivre dans le même
processus que le serveur.

Deux détails qui décident de la différence entre protéger une position et
élargir son risque sans le voir :

**Un stop n'est jamais reculé.** La comparaison est explicite par sens — sur
un achat seul un stop plus haut est une amélioration, sur une vente c'est
l'inverse.

**La cible repart avec le stop.** `TRADE_ACTION_SLTP` remplace les deux d'un
coup : omettre la cible ne la laisse pas en place, il l'efface. Une remontée
de stop qui supprimerait la cible transformerait une jambe destinée à sortir
à 4 104 en position sans sortie.

Le décalage par rapport à l'entrée est réglable. Zéro par défaut, mais zéro
n'est pas tout à fait le point mort : sur un achat, l'entrée s'est faite à
l'ask et le stop se déclenche sur le bid, donc sortir à l'entrée exacte coûte
encore le spread.

## Tests

Le dimensionnement et les garde-fous ne dépendent ni de MT5 ni du réseau, donc
ils se testent intégralement :

```powershell
py -3.12 test_plan.py
py -3.12 test_guards.py
py -3.12 test_orders.py
py -3.12 test_watcher.py
```

Cent huit cas, dont chaque refus. Un garde-fou qu'on ne sait pas déclencher en
test est un garde-fou qu'on découvrira cassé le jour où il devait servir.

Les envois se testent avec un faux MT5 : ce qu'il faut vérifier n'est pas que
le broker réponde, c'est que **la requête formée soit la bonne** — le mode
d'exécution, le type d'ordre déduit, la présence du stop dès l'entrée,
l'heure d'expiration.

## Deux pièges du modèle de données MT5

**MT5 ne stocke pas des trades mais des *deals*.** Une entrée et une sortie
font deux lignes, et une position sortie en trois fois en fait quatre.
`get_history` les regroupe par position, reconstruit les prix d'entrée et de
sortie pondérés par le volume, et additionne les frais sur l'ensemble des
deals — une sortie en trois fois paie trois commissions, et un P&L qui
l'ignorerait surévaluerait précisément les trades que ce projet produit.

**Les horodatages MT5 sont en heure du serveur du broker**, présentés comme
une époque Unix. Les prendre pour de l'UTC décale un trade de deux ou trois
heures : assez pour ranger une entrée de 08h15 dans la mauvaise session, et en
bord de journée à la mauvaise date.

Le pont mesure l'écart sur l'ouverture de la bougie M1 en cours, qui en séance
a moins d'une minute, et l'arrondit au quart d'heure. Hors séance il n'y a pas
de bougie fraîche : la dernière mesure est réutilisée, et si aucune n'a jamais
eu lieu, `time_basis` vaut `server` au lieu de `utc`. Le pont le dit plutôt
que de renvoyer une valeur plausible et fausse.

## Correspondance des symboles

Les brokers renomment. Chez XM, le Nasdaq s'appelle `US100`, et tout porte un
suffixe `#` lié au type de compte :

| Journal | XM |
|---|---|
| `GER40` | `GER40Cash#` |
| `US30` | `US30Cash#` |
| `NAS100` | `US100Cash#` |
| `XAUUSD` | `GOLD#` |

La table `[account.symbols]` fait la traduction dans les deux sens. Les outils
acceptent l'une ou l'autre orthographe et renvoient les deux, sous `symbol`
pour le broker et `journal_symbol` pour le journal. Un nom absent de la table
est transmis tel quel au terminal, qui répondra avec sa propre erreur.

## Dimensionnement

`volume_step` varie d'un instrument à l'autre chez un même broker — 0,1 sur
les indices et 0,01 sur l'or chez XM. Il se lit, il ne se suppose pas : un
arrondi au dixième sur l'or produirait des positions dix fois trop grosses.

`min_volume_for_three_legs` donne la plus petite position encore découpable en
trois jambes, soit trois fois `volume_min`.

`filling_modes` liste les politiques d'exécution que le symbole accepte. Chez
XM c'est `IOC` seul : un ordre marché envoyé en `FOK` est rejeté par le
serveur, pas corrigé.

`stops_level` à zéro signifie qu'aucune distance minimale n'est imposée
*aujourd'hui*. Plusieurs brokers en appliquent une dynamiquement autour des
publications économiques, donc un plan doit savoir encaisser un stop refusé
plutôt que se fier à cette valeur.
