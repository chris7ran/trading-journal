"""Bot Telegram : tu transfères un signal, il chiffre, tu confirmes.

    py -3.12 bot.py

Fonctionne en **long polling** : ce service appelle Telegram, Telegram ne
rappelle jamais. Aucun port ouvert, aucun webhook, aucun nom de domaine —
cohérent avec le reste de l'installation, où rien n'est exposé.

Il ne lit aucun canal. Un bot Telegram ne peut pas rejoindre un canal dont il
n'est pas membre, et personne ne t'en fera administrateur. Le transfert
manuel n'est donc pas une limite contournée faute de mieux : c'est le seul
chemin possible avec un bot, et il a le mérite d'être lui-même le premier
contrôle humain.

**Le bot ne décide rien.** Il lit le message, demande un chiffrage au pont,
affiche le verdict des garde-fous et attend un appui. Un message reçu est une
**donnée**, jamais une commande : un canal tiers ne doit pas pouvoir déclencher
un ordre parce que quelqu'un y a écrit la bonne phrase.

Configuration, par l'environnement :

    TELEGRAM_BOT_TOKEN    le jeton donné par BotFather
    TELEGRAM_OWNER_ID     ton identifiant Telegram ; tout autre est ignoré
    MCP_MT5_URL           http://127.0.0.1:8765/mcp
    MCP_MT5_TOKEN         le jeton du pont
    BOT_ACCOUNTS          alias séparés par des virgules ; par défaut, tous
                          ceux que le pont expose. `BOT_ACCOUNT` au singulier
                          reste accepté.
    BOT_RISK_PCT          risque par plan, par défaut le plafond du compte

Avec plusieurs comptes — plusieurs brokers, ou une démo à côté d'un compte
financé — un signal est chiffré sur chacun et la proposition porte une rangée
de boutons par compte. Le choix se fait donc en voyant les chiffres de chacun,
plutôt qu'en se souvenant lequel était actif.

Chaque compte garde ses propres garde-fous, et il n'existe **aucun plafond
agrégé** : appuyer sur deux comptes ouvre deux positions, et le risque
s'additionne.
"""

from __future__ import annotations

import json
import os
import secrets
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field

import mcpclient
import signals

API = "https://api.telegram.org/bot{token}/{method}"

#: Au-delà, une proposition n'est plus offerte à la confirmation. Les prix
#: seront de toute façon recalculés à l'envoi, donc ce délai ne protège pas
#: des chiffres périmés — il évite qu'un appui distrait sur un vieux message
#: ouvre une position dont le contexte a disparu.
PROPOSITION_TTL = 15 * 60

#: Attente du long polling. Telegram garde la connexion ouverte jusque-là s'il
#: n'a rien à dire, ce qui évite d'interroger en boucle pour rien.
POLL_TIMEOUT = 30


@dataclass
class Proposition:
    signal: signals.Signal | None
    entry: float
    cree: float
    #: Les comptes offerts, dans l'ordre des boutons. L'index voyage dans le
    #: `callback_data` : un bouton doit désigner son compte sans dépendre de
    #: l'ordre de configuration au moment où il est pressé.
    comptes: list[str]
    #: Ceux sur lesquels quelque chose est déjà parti. Un second appui sur le
    #: même bouton — doigt qui glisse, interface qui n'a pas rafraîchi — ne
    #: doit pas ouvrir une seconde position.
    envoyes: set[str] = field(default_factory=set)


class Bot:
    def __init__(self, token: str, owner: int, client: mcpclient.Client,
                 comptes: list[str], risque: float | None) -> None:
        self.token = token
        self.owner = owner
        self.client = client
        self.comptes = comptes
        self.risque = risque
        self.propositions: dict[str, Proposition] = {}
        self.offset = 0

    # --- Boucle -------------------------------------------------------------

    def run(self) -> None:
        print(f"bot à l'écoute, compte(s) {', '.join(self.comptes)}", flush=True)
        attente = 1
        while True:
            try:
                for update in self._updates():
                    self._sans_silence(update)
                attente = 1
            except KeyboardInterrupt:
                return
            except Exception as exc:  # noqa: BLE001
                # Un bot qui meurt sur un accroc réseau est pire qu'absent :
                # on le croit en train d'écouter.
                message = str(exc)
                if "409" in message:
                    # Telegram ne sert qu'un seul interrogateur par bot.
                    print(
                        "[bot] une autre instance interroge déjà ce bot. "
                        "Fermez-la : à deux, chacune vole les messages de "
                        "l'autre et Telegram finit par limiter les deux.",
                        flush=True,
                    )
                else:
                    print(f"[bot] {type(exc).__name__}: {message}", flush=True)
                # Attente croissante : réessayer toutes les secondes après un
                # « too many requests » ne fait que confirmer à Telegram qu'on
                # en demande trop.
                time.sleep(attente)
                attente = min(attente * 2, 60)

    def _updates(self) -> list[dict]:
        reponse = self._api(
            "getUpdates", {"offset": self.offset, "timeout": POLL_TIMEOUT},
            timeout=POLL_TIMEOUT + 10,
        )
        updates = reponse.get("result", [])
        if updates:
            self.offset = updates[-1]["update_id"] + 1
        return updates

    # --- Traitement ---------------------------------------------------------

    def _sans_silence(self, update: dict) -> None:
        """Traiter un message en garantissant qu'une réponse part toujours.

        Le traitement n'attrapait que les erreurs du pont. Toute autre —
        une clé absente dans une réponse, un champ au mauvais type — remontait
        à la boucle, était consignée dans le terminal, et l'expéditeur
        n'apprenait rien.

        Un outil de trading qui ne répond pas est pire qu'un outil qui refuse.
        On reste devant son téléphone à se demander si l'ordre est parti.
        """
        try:
            self._traiter(update)
        except Exception as exc:  # noqa: BLE001
            chat = (
                ((update.get("message") or {}).get("chat") or {}).get("id")
                or (((update.get("callback_query") or {}).get("message") or {})
                    .get("chat") or {}).get("id")
            )
            print(f"[bot] {type(exc).__name__}: {exc}", flush=True)
            if chat:
                self._envoyer(chat, f"Erreur interne : {type(exc).__name__} — {exc}")

    def _traiter(self, update: dict) -> None:
        if "callback_query" in update:
            self._bouton(update["callback_query"])
            return

        message = update.get("message") or update.get("channel_post")
        if not message:
            return

        expediteur = (message.get("from") or {}).get("id")
        if expediteur != self.owner:
            # Silence délibéré : répondre confirmerait l'existence du bot à
            # qui l'a trouvé par hasard.
            return

        texte = message.get("text") or message.get("caption") or ""
        chat = message["chat"]["id"]

        if texte.strip().startswith("/"):
            self._commande(chat, texte.strip())
            return
        if _demande_fermeture(texte):
            # Avant l'analyseur de signaux : « ferme tout » n'a ni sens ni
            # instrument, et serait rejeté comme un message illisible alors
            # que c'est l'ordre le plus clair qu'on puisse donner.
            self._fermer(chat)
            return
        if message.get("photo") and not texte.strip():
            self._envoyer(
                chat,
                "Image reçue, mais je ne sais pas encore lire les captures. "
                "Transfère le signal en texte, ou recopie entrée, stop et cibles.",
            )
            return

        self._chiffrer(chat, texte)

    def _commande(self, chat: int, texte: str) -> None:
        if texte.startswith("/start") or texte.startswith("/aide"):
            self._envoyer(
                chat,
                "Transfère-moi un signal et je le chiffre : taille de position, "
                "découpage en jambes, ratio et verdict des garde-fous.\n\n"
                "Rien n'est envoyé sans que tu appuies sur un bouton.\n\n"
                "« ferme tout » ou /fermer pour solder ce qui est ouvert.\n"
                "/etat pour le solde et les positions.\n\n"
                f"Compte(s) : {', '.join(self.comptes)}",
            )
        elif texte.startswith("/etat"):
            self._etat(chat)
        elif texte.startswith("/fermer"):
            self._fermer(chat)
        else:
            self._envoyer(chat, "Commandes : /aide, /etat, /fermer")

    def _etat(self, chat: int) -> None:
        blocs = []
        for alias in self.comptes:
            try:
                compte = self.client.call("get_account", {"account": alias})
                positions = self.client.call("get_positions", {"account": alias})
            except mcpclient.McpError as exc:
                # Un compte muet ne doit pas emporter les autres : c'est
                # justement quand un terminal décroche qu'on a besoin de voir
                # l'état de ceux qui tiennent encore.
                blocs.append(f"*{alias}* — injoignable : {exc}")
                continue

            lignes = [
                f"*{alias}* — {compte['balance']:,.0f} {compte['currency']}".replace(",", " "),
                f"équité {compte['equity']:,.2f} · marge libre {compte['margin_free']:,.0f}".replace(",", " "),
                f"{positions['count']} position(s) ouverte(s)",
            ]
            for p in positions["positions"]:
                lignes.append(
                    f"· {p['journal_symbol']} {p['direction']} {p['volume']} "
                    f"@ {p['open_price']} → {p['profit']:+.2f}"
                )
            blocs.append("\n".join(lignes))

        self._envoyer(chat, "\n\n".join(blocs))

    def _fermer(self, chat: int, magic: int | None = None) -> None:
        """Proposer la fermeture : la liste d'abord, l'action après.

        Afficher ce qui va être fermé avant de le fermer n'est pas une
        précaution de principe. « Tout fermer » inclut ce qui a été ouvert à
        la main dans le terminal, et c'est exactement le genre de position
        qu'on oublie avoir.
        """
        blocs: list[str] = []
        ouverts: list[str] = []
        for alias in self.comptes:
            try:
                positions = self.client.call("get_positions", {"account": alias})
            except mcpclient.McpError as exc:
                blocs.append(f"*{alias}* — injoignable : {exc}")
                continue
            if not positions["count"]:
                continue

            ouverts.append(alias)
            lignes = [f"*{positions['count']} position(s)* sur {alias}", ""]
            flottant = 0.0
            for p in positions["positions"]:
                flottant += p["profit"]
                origine = f"plan {p['magic']}" if p["magic"] else "ouverte à la main"
                lignes.append(
                    f"· {p['journal_symbol']} {p['direction']} {p['volume']} "
                    f"@ {p['open_price']} → {p['profit']:+.2f}  _{origine}_"
                )
            lignes += ["", f"Flottant total : *{flottant:+.2f}*"]
            blocs.append("\n".join(lignes))

        if not ouverts:
            self._envoyer(chat, "\n\n".join(["Aucune position ouverte.", *blocs]))
            return

        jeton = secrets.token_urlsafe(6)
        self.propositions[jeton] = Proposition(None, 0.0, time.time(), ouverts)
        annuler = {"text": "Annuler", "callback_data": f"n:{jeton}:0"}

        if len(ouverts) == 1:
            rangees = [[
                {"text": "Tout fermer", "callback_data": f"f:{jeton}:0"},
                annuler,
            ]]
        else:
            # Une rangée par compte, et jamais de « tout fermer partout » :
            # solder deux brokers d'un seul appui est une action qu'on ne
            # devrait pas pouvoir déclencher sans la nommer deux fois.
            rangees = [
                [{"text": f"Tout fermer · {a}", "callback_data": f"f:{jeton}:{i}"}]
                for i, a in enumerate(ouverts)
            ]
            rangees.append([annuler])

        self._envoyer(chat, "\n\n".join(blocs), boutons={"inline_keyboard": rangees})

    def _chiffrer(self, chat: int, texte: str) -> None:
        try:
            signal = signals.parse(texte)
        except signals.SignalError as exc:
            self._envoyer(chat, f"Signal illisible : {exc}")
            return

        # Le premier prix écrit : la convention de Scale FX, confirmée par
        # leur propre guide — « all 3 TP = around +1R » n'est vrai que depuis
        # ce bord-là.
        entree = signal.entry_first

        apercus: list[tuple[str, dict]] = []
        for alias in self.comptes:
            arguments = {
                "account": alias,
                "symbol": signal.symbol,
                "direction": signal.direction,
                "stop_loss": signal.stop_loss,
                "targets": signal.targets,
                "entry": entree,
            }
            if self.risque:
                arguments["risk_pct"] = self.risque
            try:
                apercus.append((alias, self.client.call("preview_plan", arguments)))
            except mcpclient.McpError as exc:
                apercus.append((alias, {"erreur": str(exc)}))

        if len(apercus) == 1:
            alias, apercu = apercus[0]
            if "erreur" in apercu:
                self._envoyer(chat, f"Chiffrage impossible : {apercu['erreur']}")
                return
            if apercu.get("decision") == "refusé" and "plan" not in apercu:
                message = f"Refusé : {apercu.get('reason', 'sans raison donnée')}"
                if apercu.get("hint"):
                    message += f"\n\n{apercu['hint']}"
                self._envoyer(chat, message)
                return

            jeton = secrets.token_urlsafe(8)
            self.propositions[jeton] = Proposition(signal, entree, time.time(), [alias])
            self._purger()
            self._envoyer(
                chat,
                _rendre(signal, apercu, entree),
                boutons=_boutons(jeton, apercu["decision"], 0),
            )
            return

        jeton = secrets.token_urlsafe(8)
        self.propositions[jeton] = Proposition(
            signal, entree, time.time(), [a for a, _ in apercus]
        )
        self._purger()
        self._envoyer(
            chat,
            _rendre_comptes(signal, apercus, entree),
            boutons=_boutons_comptes(jeton, apercus),
        )

    def _bouton(self, requete: dict) -> None:
        donnee = requete.get("data", "")
        chat = requete["message"]["chat"]["id"]
        try:
            self._api("answerCallbackQuery", {"callback_query_id": requete["id"]})
        except RuntimeError:
            # « query is too old » : le bouton a été pressé avant un
            # redémarrage. Cet appel ne sert qu'à éteindre le sablier de
            # l'interface ; son échec ne doit pas empêcher de traiter la
            # demande, qui est toujours valable.
            pass

        if (requete.get("from") or {}).get("id") != self.owner:
            return

        action, _, reste = donnee.partition(":")
        jeton, _, rang = reste.partition(":")
        proposition = self.propositions.get(jeton)
        if proposition is None:
            self._envoyer(chat, "Proposition expirée. Retransfère le signal.")
            return

        alias = proposition.comptes[int(rang)] if rang.isdigit() and int(rang) < len(
            proposition.comptes
        ) else proposition.comptes[0]

        if action == "n":
            del self.propositions[jeton]
            self._envoyer(chat, "Ignoré.")
            return
        if action == "f":
            del self.propositions[jeton]
            try:
                resultat = self.client.call(
                    "close_positions", {"account": alias, "confirm": True}
                )
            except mcpclient.McpError as exc:
                self._envoyer(chat, f"Échec : {exc}")
                return
            self._envoyer(chat, f"*{alias}*\n{_rendre_fermeture(resultat)}")
            return

        if alias in proposition.envoyes:
            # Les boutons restent affichés après un envoi réussi : Telegram ne
            # les retire pas et les laisser vivants sert justement à offrir
            # l'autre compte. Il faut donc refuser ici, pas espérer que
            # personne ne réappuie.
            self._envoyer(chat, f"Déjà envoyé sur *{alias}*. Retransfère le signal pour un second plan.")
            return

        signal = proposition.signal
        arguments = {
            "account": alias,
            "symbol": signal.symbol,
            "direction": signal.direction,
            "stop_loss": signal.stop_loss,
            "targets": signal.targets,
            "confirm": True,
        }
        if self.risque:
            arguments["risk_pct"] = self.risque

        if action == "a":  # attendre : déposer au niveau prévu
            arguments["entry"] = proposition.entry
            outil = "schedule_plan"
        else:
            arguments["entry"] = proposition.entry
            outil = "execute_plan"

        try:
            resultat = self.client.call(outil, arguments)
        except mcpclient.McpError as exc:
            self._envoyer(chat, f"Échec : {exc}")
            return

        # Un refus doit laisser l'autre bouton utilisable : le 09/11, un plan
        # refusé au marché pour cause de ratio dégradé aurait été accepté en
        # ordre déposé, puisque le dimensionnement part alors du niveau visé.
        # Obliger à retransférer le signal pour essayer l'autre voie, c'était
        # perdre les secondes qui font justement la différence.
        #
        # Un succès ne consomme plus la proposition entière, seulement le
        # compte servi : les rangées des autres comptes doivent survivre à
        # l'envoi sur le premier, sinon le choix offert n'en est pas un.
        if resultat.get("sent"):
            proposition.envoyes.add(alias)

        prefixe = f"*{alias}*\n" if len(proposition.comptes) > 1 else ""
        self._envoyer(
            chat,
            prefixe + _rendre_envoi(resultat),
            boutons=None if resultat.get("sent")
            else _boutons(jeton, "à confirmer", proposition.comptes.index(alias)),
        )

    def _purger(self) -> None:
        limite = time.time() - PROPOSITION_TTL
        for jeton in [j for j, p in self.propositions.items() if p.cree < limite]:
            del self.propositions[jeton]

    # --- Telegram -----------------------------------------------------------

    def _envoyer(self, chat: int, texte: str, boutons: dict | None = None) -> None:
        """Envoyer, en retombant sur du texte brut si la mise en forme casse.

        Telegram refuse **tout le message** si son analyse Markdown trouve une
        étoile ou un tiret bas non apparié. Or les contenus dynamiques en
        apportent sans prévenir : un nom de symbole, un message d'erreur, une
        note d'émetteur. Le résultat est le pire possible — aucune réponse, là
        où on attend un chiffrage.

        La mise en forme n'est donc pas un acquis mais une tentative. Si elle
        échoue, le même texte repart sans balises : moins lisible, et infiniment
        préférable au silence.
        """
        charge = {"chat_id": chat, "text": texte, "parse_mode": "Markdown"}
        if boutons:
            charge["reply_markup"] = json.dumps(boutons)
        try:
            self._api("sendMessage", charge)
        except RuntimeError as exc:
            if "parse entities" not in str(exc):
                raise
            charge.pop("parse_mode")
            # Sans `parse_mode`, Telegram n'interprète plus rien : les étoiles
            # que *nous* avons ajoutées pour l'emphase n'ont plus de sens et
            # s'enlèvent, mais les tirets bas appartiennent aux données. Les
            # retirer aussi transformait `schedule_plan` en « scheduleplan »
            # et `order_send` en « ordersend » — des noms qui n'existent pas,
            # dans le message même où on cherche ce qui a cassé.
            charge["text"] = texte.replace("*", "")
            self._api("sendMessage", charge)

    def _api(self, methode: str, parametres: dict, timeout: int = 30) -> dict:
        url = API.format(token=self.token, method=methode)
        donnees = urllib.parse.urlencode(parametres).encode()
        requete = urllib.request.Request(url, data=donnees, method="POST")
        try:
            with urllib.request.urlopen(requete, timeout=timeout) as reponse:
                return json.loads(reponse.read().decode())
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")[:200]
            raise RuntimeError(f"Telegram HTTP {exc.code} : {detail}") from exc


# --- Mise en forme ---------------------------------------------------------------


def _rendre(signal: signals.Signal, apercu: dict, entree: float) -> str:
    plan = apercu.get("plan")
    if not plan:
        return f"Refusé : {apercu.get('reason', '')}"

    lignes = [
        f"*{signal.symbol} {signal.direction}* — {apercu['decision'].upper()}",
        "",
        f"entrée prévue {entree} · marché {plan['reference_price']}",
        f"stop {plan['stop_loss']} ({plan['stop_distance']})",
        f"*{plan['total_volume']} lot* pour {plan['risk_amount']:,.0f} "
        f"{plan['currency']} ({plan['risk_pct']:.2%})".replace(",", " "),
        "",
        f"R pondéré *{plan['blended_r']}* — zone {plan['zone']}",
    ]
    if plan.get("degradation"):
        lignes.append(f"soit {plan['degradation']:.0%} d'espérance perdue depuis l'entrée prévue")

    lignes.append("")
    for jambe in plan["legs"]:
        lignes.append(
            f"· {jambe['volume']} → {jambe['take_profit']} ({jambe['r_multiple']} R)"
        )

    if apercu.get("findings"):
        lignes.append("")
        for f in apercu["findings"]:
            marque = {"block": "⛔", "warn": "⚠️", "info": "ℹ️"}.get(f["level"], "·")
            lignes.append(f"{marque} {f['message']}")

    attente = apercu.get("wait_instead")
    if attente:
        lignes += ["", f"_{attente['reading']}_"]

    if signal.notes:
        lignes += ["", "_" + " · ".join(signal.notes) + "_"]

    return "\n".join(lignes)


def _rendre_comptes(
    signal: signals.Signal, apercus: list[tuple[str, dict]], entree: float
) -> str:
    """Le signal une fois, puis une ligne de synthèse par compte.

    Répéter les jambes et les constats de chaque compte donnerait un pavé que
    personne ne lit sur un téléphone, au moment précis où il faut décider
    vite. Ne survit ici que ce qui diffère réellement d'un compte à l'autre —
    le volume, le risque en devise, le ratio et le verdict — plus les constats
    bloquants, parce qu'un compte refusé doit dire pourquoi sans qu'on ait à
    le demander.
    """
    lignes = [
        f"*{signal.symbol} {signal.direction}*",
        f"entrée prévue {entree} · stop {signal.stop_loss}",
        "cibles " + " / ".join(str(c) for c in signal.targets),
        "",
    ]

    for alias, apercu in apercus:
        if "erreur" in apercu:
            lignes.append(f"*{alias}* — chiffrage impossible : {apercu['erreur']}")
            lignes.append("")
            continue

        plan = apercu.get("plan")
        if not plan:
            lignes.append(
                f"*{alias}* — refusé : {apercu.get('reason', 'sans raison donnée')}"
            )
            lignes.append("")
            continue

        lignes.append(f"*{alias}* — {apercu['decision'].upper()}")
        lignes.append(
            f"{plan['total_volume']} lot · {plan['risk_amount']:,.0f} "
            f"{plan['currency']} ({plan['risk_pct']:.2%}) · R {plan['blended_r']} "
            f"({plan['zone']})".replace(",", " ")
        )
        for f in apercu.get("findings", []):
            if f["level"] == "block":
                lignes.append(f"⛔ {f['message']}")
        lignes.append("")

    if signal.notes:
        lignes.append("_" + " · ".join(signal.notes) + "_")

    return "\n".join(lignes).rstrip()


def _boutons_comptes(jeton: str, apercus: list[tuple[str, dict]]) -> dict:
    """Une rangée par compte chiffré, et rien pour ceux qui n'ont pas de plan.

    Un compte dont le terminal n'a pas répondu, ou dont le signal porte un
    symbole qu'il ne connaît pas, n'a pas de bouton du tout. Offrir l'action
    puis échouer à l'appui ferait croire à une panne du pont alors que le
    chiffrage avait déjà dit non.
    """
    rangees = []
    for rang, (alias, apercu) in enumerate(apercus):
        if "erreur" in apercu or not apercu.get("plan"):
            continue
        depot = {"text": f"Déposer · {alias}", "callback_data": f"a:{jeton}:{rang}"}
        if apercu["decision"] == "refusé":
            rangees.append([depot])
            continue
        envoyer = "Exécuter" if apercu["decision"] == "accepté" else "Exécuter quand même"
        rangees.append([
            {"text": f"{envoyer} · {alias}", "callback_data": f"x:{jeton}:{rang}"},
            depot,
        ])

    rangees.append([{"text": "Ignorer", "callback_data": f"n:{jeton}:0"}])
    return {"inline_keyboard": rangees}


def _boutons(jeton: str, decision: str, rang: int) -> dict:
    """Les actions offertes, selon le verdict.

    Un plan refusé garde le dépôt. Le refus porte sur l'entrée **au marché** :
    déposer un ordre au niveau prévu est une autre opération, dimensionnée
    depuis ce niveau-là, et c'est précisément quand le marché s'est éloigné
    qu'elle devient le bon choix.

    Le 09/11, un plan est sorti refusé avec, dans le même message, « attendre
    bat entrer si le prix revient avec au moins 10,5 % de chances » — et aucun
    bouton pour le faire. Un système qui dit quoi faire sans permettre de le
    faire est pire qu'un système muet : il donne le sentiment d'avoir décidé.
    """
    depot = {"text": "Déposer et attendre", "callback_data": f"a:{jeton}:{rang}"}
    ignorer = {"text": "Ignorer", "callback_data": f"n:{jeton}:{rang}"}

    if decision == "refusé":
        return {"inline_keyboard": [[depot, ignorer]]}

    envoyer = "Exécuter" if decision == "accepté" else "Exécuter quand même"
    return {
        "inline_keyboard": [[
            {"text": envoyer, "callback_data": f"x:{jeton}:{rang}"},
            depot,
            ignorer,
        ]]
    }


#: Ce qui vaut « ferme », dans les formulations qui viennent naturellement
#: quand on veut sortir vite. Volontairement large : à ce moment-là, on
#: n'écrit pas une commande, on écrit ce qui vient.
FERMETURE = (
    "ferme", "fermer", "close", "sors", "sortir", "solde", "solder",
    "cloture", "clôture", "cut", "flat",
)


def _demande_fermeture(texte: str) -> bool:
    mots = texte.lower().split()
    return bool(mots) and any(m.strip(".,!?") in FERMETURE for m in mots[:3])


def _rendre_fermeture(resultat: dict) -> str:
    if resultat.get("needs_confirmation"):
        return resultat.get("message", "confirmation requise")
    fermees = resultat.get("closed", [])
    if not fermees and not resultat.get("failed"):
        return "Aucune position à fermer."

    lignes = [f"*{len(fermees)} position(s) fermée(s)*"]
    for f in fermees:
        lignes.append(f"· {f['symbol']} {f['volume']} @ {f.get('price')} → {f['profit']:+.2f}")
    if resultat.get("realised") is not None:
        lignes.append(f"\nRéalisé : *{resultat['realised']:+.2f}*")
    for e in resultat.get("failed", []):
        lignes.append(f"⛔️ ticket {e['ticket']} : {e['error']}")
    return "\n".join(lignes)


def _rendre_envoi(resultat: dict) -> str:
    if not resultat.get("sent"):
        motifs = "\n".join(f"· {f['message']}" for f in resultat.get("findings", []))
        return f"*Non envoyé* — {resultat.get('decision')}\n{motifs}"

    execution = resultat.get("execution", {})
    lignes = [
        f"*Envoyé* — plan {execution.get('magic')}",
        f"{len(execution.get('legs', []))} jambe(s), mode {execution.get('mode')}",
    ]
    for jambe in execution.get("legs", []):
        lignes.append(f"· jambe {jambe['leg']} : {jambe['volume']} @ {jambe['price']}")
    if execution.get("expires_at_server"):
        lignes.append(f"expire à {execution['expires_at_server']} (heure serveur)")
    return "\n".join(lignes)


# --- Démarrage ------------------------------------------------------------------


def _decouvrir(token: str) -> int:
    """Afficher l'identifiant de qui écrit au bot, puis s'arrêter.

    `TELEGRAM_OWNER_ID` sert à ce que le bot n'obéisse qu'à une personne, et
    c'est un nombre que Telegram ne montre nulle part dans son interface. Le
    demander à un bot tiers marche, mais ajoute un service à la chaîne pour
    une information que le bot peut lire lui-même.

    Aucun ordre ne peut partir d'ici : cette fonction ne touche ni au pont ni
    à MT5, elle lit les messages reçus et les affiche.
    """
    print("Envoyez n'importe quel message à votre bot depuis Telegram…", flush=True)
    offset = 0
    fin = time.time() + 120

    while time.time() < fin:
        url = API.format(token=token, method="getUpdates")
        donnees = urllib.parse.urlencode({"offset": offset, "timeout": 20}).encode()
        try:
            with urllib.request.urlopen(
                urllib.request.Request(url, data=donnees, method="POST"), timeout=30
            ) as reponse:
                resultat = json.loads(reponse.read().decode()).get("result", [])
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")[:200]
            print(f"Telegram refuse le jeton : HTTP {exc.code} — {detail}")
            return 2

        for update in resultat:
            offset = update["update_id"] + 1
            auteur = ((update.get("message") or {}).get("from")) or {}
            if auteur.get("id"):
                nom = auteur.get("first_name", "")
                pseudo = f" @{auteur['username']}" if auteur.get("username") else ""
                print(f"\nIdentifiant trouvé : {auteur['id']}  ({nom}{pseudo})\n")
                print("Enregistrez-le, puis relancez le bot :")
                print(
                    '  [Environment]::SetEnvironmentVariable('
                    f'"TELEGRAM_OWNER_ID", "{auteur["id"]}", "User")'
                )
                return 0

    print("Aucun message reçu en deux minutes. Le bot a-t-il été démarré "
          "dans Telegram ?")
    return 1


def main() -> int:
    token = os.environ.get("TELEGRAM_BOT_TOKEN", "").strip()
    owner = os.environ.get("TELEGRAM_OWNER_ID", "").strip()

    if not token:
        print(
            "TELEGRAM_BOT_TOKEN est requis : écrivez /newbot à @BotFather, ou\n"
            "/mybots puis « API Token » si le bot existe déjà."
        )
        return 2

    if not owner.isdigit():
        # Plutôt que de renvoyer vers un bot tiers pour obtenir un numéro :
        # le bot le demande au sien. Un service de moins dans la chaîne, et
        # une étape de moins à expliquer.
        return _decouvrir(token)

    client = mcpclient.Client(
        os.environ.get("MCP_MT5_URL", "http://127.0.0.1:8765/mcp"),
        os.environ.get("MCP_MT5_TOKEN", ""),
    )
    try:
        client.connect()
        comptes = client.call("list_accounts")
    except mcpclient.McpError as exc:
        print(f"pont injoignable : {exc}")
        return 2

    configures = [c["alias"] for c in comptes.get("accounts", [])]
    if not configures:
        print("aucun compte sur le pont")
        return 2

    demande = (
        os.environ.get("BOT_ACCOUNTS", "").strip()
        or os.environ.get("BOT_ACCOUNT", "").strip()
    )
    choisis = [a.strip() for a in demande.split(",") if a.strip()]
    # Un alias mal orthographié doit arrêter le démarrage, pas se traduire en
    # silence par un compte de moins : c'est le genre d'erreur qu'on ne
    # remarque que le jour où le signal n'est pas passé là où on croyait.
    inconnus = [a for a in choisis if a not in configures]
    if inconnus:
        print(f"compte(s) inconnu(s) du pont : {', '.join(inconnus)}")
        print(f"configurés : {', '.join(configures)}")
        return 2

    risque = os.environ.get("BOT_RISK_PCT", "").strip()
    Bot(
        token=token,
        owner=int(owner),
        client=client,
        comptes=choisis or configures,
        risque=float(risque) if risque else None,
    ).run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
