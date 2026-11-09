"""Remontée des trades exécutés vers le journal.

Le pont pousse, le journal range. Rien n'est gardé en mémoire ici : chaque
tour renvoie l'état courant des plans, et le backend fait un upsert sur le
ticket. Un envoi perdu est donc rattrapé au tour suivant, et un redémarrage
ne laisse aucun trou — c'est ce qui permet au watcher de ne tenir aucun
registre et de rester sans état.

**Le symbole poussé est celui du journal, pas celui du broker.** `GOLD#` chez
XM, `XAUUSD` partout ailleurs : pousser le nom du broker ferait apparaître
deux instruments distincts dans les statistiques, et les trades sur l'or
seraient coupés en deux séries selon le terminal qui les a produits.

**L'identifiant de plan est le magic**, pas l'UUID. Une position MT5 ne porte
que 31 caractères de commentaire, donc l'UUID n'y tient pas en entier. Le
magic, lui, est présent sur chaque position et chaque deal, et c'est le seul
identifiant que le broker connaisse vraiment. L'UUID reste dans le journal
d'audit pour faire le lien le jour où l'on enquête.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request


class JournalError(Exception):
    """Le journal n'a pas pris les trades. Signalé, jamais fatal."""


class Journal:
    """Client du backend, limité à l'ingestion de trades."""

    def __init__(self, url: str, token: str, *, timeout: int = 20) -> None:
        self.url = url.rstrip("/")
        self.token = token
        self.timeout = timeout

    def push(self, login: str, trades: list[dict]) -> dict:
        """Envoyer un lot. Lève `JournalError` sur tout échec."""
        if not trades:
            return {"received": 0, "created": 0, "updated": 0}

        corps = json.dumps({"login": login, "trades": trades}).encode()
        requete = urllib.request.Request(
            f"{self.url}/ingest/trades", data=corps, method="POST"
        )
        requete.add_header("Content-Type", "application/json")
        requete.add_header("X-Ingest-Token", self.token)

        try:
            with urllib.request.urlopen(requete, timeout=self.timeout) as reponse:
                return json.loads(reponse.read().decode())
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")[:300]
            raise JournalError(f"HTTP {exc.code} : {detail}") from exc
        except urllib.error.URLError as exc:
            raise JournalError(f"journal injoignable ({self.url}) : {exc.reason}") from exc


def from_execution(
    account,
    sized,
    execution: dict,
    magic: int,
) -> list[dict]:
    """Les jambes d'un plan qui vient d'être envoyé, prêtes pour le journal.

    Pourquoi ne pas laisser le watcher s'en charger au tour suivant : vingt
    secondes suffisent à une jambe pour vivre et mourir. Le 09/11, la première
    cible est tombée avant que la remontée ne soit configurée ; MT5 ne
    conservant pas le stop d'une position fermée, son R est devenu
    incalculable. Rien ne reconstitue après coup ce qui n'a jamais été écrit.

    Ici le stop et les cibles sont connus — ce sont ceux qu'on vient de
    transmettre — donc les écrire tout de suite ferme la fenêtre.
    """
    from datetime import datetime, timezone

    maintenant = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
    par_index = {leg.index: leg for leg in sized.legs}

    lot = []
    for envoyee in execution.get("legs", []):
        leg = par_index.get(envoyee["leg"])
        if leg is None:
            continue
        lot.append(
            {
                # Sur un ordre au marché, MT5 donne à la position
                # l'identifiant de l'ordre qui l'a ouverte : c'est ce qui
                # permettra au watcher de retrouver cette ligne plus tard.
                "ticket": str(envoyee["order"]),
                "symbol": account.to_journal(sized.symbol),
                "direction": sized.direction,
                # Horodatage approché ; le watcher le remplacera par celui du
                # terminal au tour suivant, la valeur transmise l'emportant
                # sur l'absence comme sur l'ancienne.
                "open_time": maintenant,
                "open_price": envoyee.get("price") or sized.reference_price,
                "stop_loss": sized.stop_loss,
                "take_profit": leg.take_profit,
                "lot_size": leg.volume,
                "plan_id": str(magic),
            }
        )
    return lot


def to_trades(
    positions: list[dict],
    history: list[dict],
    to_journal_symbol,
) -> list[dict]:
    """Convertir l'état du terminal en lot pour `POST /ingest/trades`.

    Les positions ouvertes et les trades clôturés partent ensemble. Les
    ouvertes portent leur stop et leur cible, les clôturées ne les ont plus :
    MT5 ne conserve pas le stop d'une position fermée. C'est sans conséquence
    parce que l'upsert du backend ne remplace jamais une valeur connue par
    une absence — le stop enregistré pendant que la position vivait survit à
    sa clôture, et le R de chaque trade reste calculable après coup.

    Ce qui n'a pas de magic est ignoré : le pont ne remonte que les plans
    qu'il a posés, pas ce que le trader ouvre à la main dans le terminal.
    """
    lot = []

    for position in positions:
        magic = position.get("magic") or 0
        if not magic:
            continue
        lot.append(
            {
                "ticket": str(position["ticket"]),
                "symbol": to_journal_symbol(position["symbol"]),
                "direction": position["direction"],
                "open_time": position.get("open_time"),
                "open_price": position.get("open_price"),
                "stop_loss": position.get("stop_loss"),
                "take_profit": position.get("take_profit"),
                "lot_size": position.get("volume"),
                "pnl": position.get("profit"),
                "swap": position.get("swap"),
                "plan_id": str(magic),
            }
        )

    for trade in history:
        magic = trade.get("magic") or 0
        if not magic or not trade.get("closed"):
            continue
        lot.append(
            {
                "ticket": str(trade["ticket"]),
                "symbol": to_journal_symbol(trade["symbol"]),
                "direction": trade["direction"],
                "open_time": trade.get("open_time"),
                "close_time": trade.get("close_time"),
                "open_price": trade.get("open_price"),
                "close_price": trade.get("close_price"),
                "lot_size": trade.get("volume"),
                "pnl": trade.get("profit"),
                "commission": trade.get("commission"),
                "swap": trade.get("swap"),
                "plan_id": str(magic),
            }
        )

    return lot
