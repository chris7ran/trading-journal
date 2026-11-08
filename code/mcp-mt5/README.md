# Pont MCP MetaTrader 5 — lecture seule

Expose un ou plusieurs terminaux MetaTrader 5 comme un serveur MCP, pour
consulter comptes, positions, historique et spécifications d'instruments
depuis un assistant.

**Ce serveur ne trade pas.** Les neuf outils lisent, aucun n'envoie d'ordre.
L'exécution viendra dans un module séparé, pour qu'elle reste une addition
visible et relisible plutôt qu'une ligne noyée dans un fichier qui savait déjà
passer des ordres.

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

Tous prennent un `account` : l'alias défini dans `accounts.toml`.

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
