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
    BOT_ACCOUNT           alias du compte, par défaut le premier configuré
    BOT_RISK_PCT          risque par plan, par défaut le plafond du compte
"""

from __future__ import annotations

import json
import os
import secrets
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass

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
    signal: signals.Signal
    entry: float
    cree: float


class Bot:
    def __init__(self, token: str, owner: int, client: mcpclient.Client,
                 compte: str, risque: float | None) -> None:
        self.token = token
        self.owner = owner
        self.client = client
        self.compte = compte
        self.risque = risque
        self.propositions: dict[str, Proposition] = {}
        self.offset = 0

    # --- Boucle -------------------------------------------------------------

    def run(self) -> None:
        print(f"bot à l'écoute, compte {self.compte}", flush=True)
        while True:
            try:
                for update in self._updates():
                    self._traiter(update)
            except KeyboardInterrupt:
                return
            except Exception as exc:  # noqa: BLE001
                # Un bot qui meurt sur un accroc réseau est pire qu'absent :
                # on le croit en train d'écouter.
                print(f"[bot] {type(exc).__name__}: {exc}", flush=True)
                time.sleep(5)

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
                f"Compte : {self.compte}\n/etat pour le solde et les positions.",
            )
        elif texte.startswith("/etat"):
            self._etat(chat)
        else:
            self._envoyer(chat, "Commandes : /aide, /etat")

    def _etat(self, chat: int) -> None:
        try:
            compte = self.client.call("get_account", {"account": self.compte})
            positions = self.client.call("get_positions", {"account": self.compte})
        except mcpclient.McpError as exc:
            self._envoyer(chat, f"Pont injoignable : {exc}")
            return

        lignes = [
            f"*{self.compte}* — {compte['balance']:,.0f} {compte['currency']}".replace(",", " "),
            f"équité {compte['equity']:,.2f} · marge libre {compte['margin_free']:,.0f}".replace(",", " "),
            f"{positions['count']} position(s) ouverte(s)",
        ]
        for p in positions["positions"]:
            lignes.append(
                f"· {p['journal_symbol']} {p['direction']} {p['volume']} "
                f"@ {p['open_price']} → {p['profit']:+.2f}"
            )
        self._envoyer(chat, "\n".join(lignes))

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

        arguments = {
            "account": self.compte,
            "symbol": signal.symbol,
            "direction": signal.direction,
            "stop_loss": signal.stop_loss,
            "targets": signal.targets,
            "entry": entree,
        }
        if self.risque:
            arguments["risk_pct"] = self.risque

        try:
            apercu = self.client.call("preview_plan", arguments)
        except mcpclient.McpError as exc:
            self._envoyer(chat, f"Chiffrage impossible : {exc}")
            return

        if apercu.get("decision") == "refusé" and "plan" not in apercu:
            self._envoyer(chat, f"Refusé : {apercu.get('reason', 'sans raison donnée')}")
            return

        jeton = secrets.token_urlsafe(8)
        self.propositions[jeton] = Proposition(signal, entree, time.time())
        self._purger()

        self._envoyer(
            chat,
            _rendre(signal, apercu, entree),
            boutons=_boutons(jeton, apercu["decision"]),
        )

    def _bouton(self, requete: dict) -> None:
        donnee = requete.get("data", "")
        chat = requete["message"]["chat"]["id"]
        self._api("answerCallbackQuery", {"callback_query_id": requete["id"]})

        if (requete.get("from") or {}).get("id") != self.owner:
            return

        action, _, jeton = donnee.partition(":")
        proposition = self.propositions.get(jeton)
        if proposition is None:
            self._envoyer(chat, "Proposition expirée. Retransfère le signal.")
            return
        if action == "n":
            del self.propositions[jeton]
            self._envoyer(chat, "Ignoré.")
            return

        signal = proposition.signal
        arguments = {
            "account": self.compte,
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

        # La proposition n'est consommée que si quelque chose est parti. Un
        # refus doit laisser l'autre bouton utilisable : le 09/11, un plan
        # refusé au marché pour cause de ratio dégradé aurait été accepté en
        # ordre déposé, puisque le dimensionnement part alors du niveau visé.
        # Obliger à retransférer le signal pour essayer l'autre voie, c'était
        # perdre les secondes qui font justement la différence.
        if resultat.get("sent"):
            self.propositions.pop(jeton, None)

        self._envoyer(
            chat,
            _rendre_envoi(resultat),
            boutons=None if resultat.get("sent") else _boutons(jeton, "à confirmer"),
        )

    def _purger(self) -> None:
        limite = time.time() - PROPOSITION_TTL
        for jeton in [j for j, p in self.propositions.items() if p.cree < limite]:
            del self.propositions[jeton]

    # --- Telegram -----------------------------------------------------------

    def _envoyer(self, chat: int, texte: str, boutons: dict | None = None) -> None:
        charge = {"chat_id": chat, "text": texte, "parse_mode": "Markdown"}
        if boutons:
            charge["reply_markup"] = json.dumps(boutons)
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


def _boutons(jeton: str, decision: str) -> dict:
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
    depot = {"text": "Déposer et attendre", "callback_data": f"a:{jeton}"}
    ignorer = {"text": "Ignorer", "callback_data": f"n:{jeton}"}

    if decision == "refusé":
        return {"inline_keyboard": [[depot, ignorer]]}

    envoyer = "Exécuter" if decision == "accepté" else "Exécuter quand même"
    return {
        "inline_keyboard": [[
            {"text": envoyer, "callback_data": f"x:{jeton}"},
            depot,
            ignorer,
        ]]
    }


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

    alias = os.environ.get("BOT_ACCOUNT", "").strip()
    if not alias:
        actifs = [c["alias"] for c in comptes.get("accounts", [])]
        if not actifs:
            print("aucun compte sur le pont")
            return 2
        alias = actifs[0]

    risque = os.environ.get("BOT_RISK_PCT", "").strip()
    Bot(
        token=token,
        owner=int(owner),
        client=client,
        compte=alias,
        risque=float(risque) if risque else None,
    ).run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
