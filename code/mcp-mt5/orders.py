"""Les seuls appels MT5 qui écrivent.

Isolés dans leur propre module, et importés par `worker.py` uniquement quand
un compte autorise l'exécution. Tout le reste du pont reste démontrablement en
lecture : chercher `order_send` dans les sources ne doit ramener qu'ici.

Un piège mérite d'être nommé d'emblée, parce qu'il échoue en silence.

**Les deux numérotations du mode d'exécution ne sont pas la même.** Le champ
`filling_mode` d'un symbole est un masque de bits — 1 pour FOK, 2 pour IOC, 4
pour BOC. L'énumération `ORDER_FILLING_*` à mettre dans la requête utilise une
tout autre échelle : FOK vaut 0, IOC vaut 1, RETURN vaut 2. Recopier le masque
dans la requête, ce que le nom identique invite à faire, envoie donc du FOK
là où le symbole n'accepte que l'IOC — et le serveur rejette sans expliquer.
Chez XM, `filling_mode` vaut 2, et la requête doit porter 1.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from typing import Any

# --- Constantes du protocole, écrites en clair -------------------------------
# Les valeurs sont fixées par MQL5. Les lire sur le module exposerait à une
# `AttributeError` au premier changement de nom du paquet Python, dans le seul
# endroit du pont où une exception se paie en argent.

TRADE_ACTION_DEAL = 1
TRADE_ACTION_PENDING = 5
TRADE_ACTION_REMOVE = 2
TRADE_ACTION_SLTP = 6

ORDER_TYPE_BUY = 0
ORDER_TYPE_SELL = 1
ORDER_TYPE_BUY_LIMIT = 2
ORDER_TYPE_SELL_LIMIT = 3
ORDER_TYPE_BUY_STOP = 4
ORDER_TYPE_SELL_STOP = 5

ORDER_TIME_GTC = 0
ORDER_TIME_SPECIFIED = 2

#: Masque porté par le symbole, qui dit quelles péremptions il accepte. Même
#: piège que `filling_mode` : c'est un masque, pas une valeur de requête.
EXPIRATION_FLAG_GTC = 1
EXPIRATION_FLAG_DAY = 2
EXPIRATION_FLAG_SPECIFIED = 4
EXPIRATION_FLAG_SPECIFIED_DAY = 8

#: Masque porté par le symbole.
FILLING_FLAG_FOK = 1
FILLING_FLAG_IOC = 2

#: Valeurs à mettre dans la requête. Rien à voir avec le masque ci-dessus.
ORDER_FILLING_FOK = 0
ORDER_FILLING_IOC = 1
ORDER_FILLING_RETURN = 2

#: Le serveur a exécuté. 10008 est la variante des ordres en attente, posés
#: et non remplis.
RETCODE_DONE = 10009
RETCODE_PLACED = 10008

#: Glissement maximum accepté, en points. Au-delà, l'ordre est rejeté plutôt
#: que rempli à un prix qui n'est plus celui sur lequel le risque a été
#: calculé — ce qui est tout l'intérêt d'en fixer un.
DEFAULT_DEVIATION = 20


class OrderError(Exception):
    """Le broker a refusé, ou la requête n'a pas pu être formée."""


def filling_for(mt5: Any, symbol: str) -> int:
    """Mode d'exécution à mettre dans la requête, lu sur le symbole."""
    info = mt5.symbol_info(symbol)
    if info is None:
        raise OrderError(f"symbole illisible : {symbol}")
    if info.filling_mode & FILLING_FLAG_IOC:
        return ORDER_FILLING_IOC
    if info.filling_mode & FILLING_FLAG_FOK:
        return ORDER_FILLING_FOK
    # Aucun des deux : le symbole n'accepte que le report, ce qui pour un
    # ordre au marché revient à ne pas être traitable ici.
    return ORDER_FILLING_RETURN


def market_leg(
    mt5: Any,
    *,
    symbol: str,
    direction: str,
    volume: float,
    stop_loss: float,
    take_profit: float,
    magic: int,
    comment: str,
    deviation: int = DEFAULT_DEVIATION,
) -> dict:
    """Une jambe au marché, stop et cible déposés avec elle.

    Le stop et la cible partent **dans le même ordre** que l'entrée, pas dans
    une modification qui suivrait. Une position ouverte une seconde sans stop
    est une position sans filet si le processus meurt entre les deux appels.
    """
    is_long = direction == "LONG"
    tick = mt5.symbol_info_tick(symbol)
    if tick is None or not (tick.bid and tick.ask):
        raise OrderError(f"aucune cotation pour {symbol}")

    request = {
        "action": TRADE_ACTION_DEAL,
        "symbol": symbol,
        "volume": float(volume),
        "type": ORDER_TYPE_BUY if is_long else ORDER_TYPE_SELL,
        "price": tick.ask if is_long else tick.bid,
        "sl": float(stop_loss),
        "tp": float(take_profit),
        "deviation": deviation,
        "magic": int(magic),
        "comment": comment[:31],
        "type_time": ORDER_TIME_GTC,
        "type_filling": filling_for(mt5, symbol),
    }
    return _send(mt5, request)


def pending_leg(
    mt5: Any,
    *,
    symbol: str,
    direction: str,
    volume: float,
    entry: float,
    stop_loss: float,
    take_profit: float,
    magic: int,
    comment: str,
    expires_at_server: datetime,
) -> dict:
    """Une jambe en attente au niveau voulu, avec expiration.

    Le type se déduit de la position du prix : un achat sous le marché est une
    limite, au-dessus c'est un stop. Le demander à l'appelant inviterait à se
    tromper sur un détail que le terminal connaît déjà.

    L'expiration est **obligatoire** et exprimée en heure serveur. Un ordre
    sans date de péremption n'est plus un plan, c'est un piège posé sur le
    marché qui se déclenchera un jour sur un mouvement sans rapport.
    """
    is_long = direction == "LONG"
    tick = mt5.symbol_info_tick(symbol)
    if tick is None or not (tick.bid and tick.ask):
        raise OrderError(f"aucune cotation pour {symbol}")

    info = mt5.symbol_info(symbol)
    if info is None:
        raise OrderError(f"symbole illisible : {symbol}")
    modes = getattr(info, "expiration_mode", EXPIRATION_FLAG_SPECIFIED)
    if not modes & EXPIRATION_FLAG_SPECIFIED:
        # Même famille de piège que `filling_mode` : le symbole dit par un
        # masque quelles péremptions il accepte. Poser une date sur un symbole
        # qui n'en veut pas fait rejeter l'ordre, et retomber silencieusement
        # sur GTC poserait justement l'ordre sans péremption que la docstring
        # ci-dessus interdit.
        acceptes = [
            nom for bit, nom in (
                (EXPIRATION_FLAG_GTC, "GTC"),
                (EXPIRATION_FLAG_DAY, "jour"),
                (EXPIRATION_FLAG_SPECIFIED_DAY, "date du jour"),
            ) if modes & bit
        ]
        raise OrderError(
            f"{symbol} n'accepte pas d'expiration datée "
            f"(accepte : {', '.join(acceptes) or 'rien'})"
        )

    reference = tick.ask if is_long else tick.bid
    if is_long:
        order_type = ORDER_TYPE_BUY_LIMIT if entry < reference else ORDER_TYPE_BUY_STOP
    else:
        order_type = ORDER_TYPE_SELL_LIMIT if entry > reference else ORDER_TYPE_SELL_STOP

    request = {
        "action": TRADE_ACTION_PENDING,
        "symbol": symbol,
        "volume": float(volume),
        "type": order_type,
        "price": float(entry),
        "sl": float(stop_loss),
        "tp": float(take_profit),
        "magic": int(magic),
        "comment": comment[:31],
        "type_time": ORDER_TIME_SPECIFIED,
        "expiration": _horodatage_mt5(expires_at_server),
        "type_filling": ORDER_FILLING_RETURN,
    }
    return _send(mt5, request)


def close_position(
    mt5: Any,
    *,
    symbol: str,
    ticket: int,
    volume: float,
    direction: str,
    magic: int = 0,
    deviation: int = DEFAULT_DEVIATION,
) -> dict:
    """Fermer une position ouverte, en tout ou en partie.

    MT5 n'a pas d'action « fermer » : on envoie un ordre marché **en sens
    inverse** portant le ticket de la position dans le champ `position`. Sans
    ce champ, en mode couverture, le même ordre ouvrirait une seconde position
    opposée au lieu de solder la première — deux positions qui se neutralisent
    en apparence, mais qui paient deux spreads et deux swaps, et qu'aucune des
    deux ne protège.

    Un `volume` inférieur à celui de la position la réduit au lieu de la
    fermer. C'est le seul endroit du pont où une fermeture partielle est
    possible, et elle n'est pas utilisée par les plans : leurs sorties
    partielles sont des jambes distinctes, déposées chez le broker.
    """
    is_long = direction.upper() in ("LONG", "BUY")
    tick = mt5.symbol_info_tick(symbol)
    if tick is None or not (tick.bid and tick.ask):
        raise OrderError(f"aucune cotation pour {symbol}")

    request = {
        "action": TRADE_ACTION_DEAL,
        "symbol": symbol,
        "volume": float(volume),
        # Sens inverse de la position : on vend ce qu'on a acheté.
        "type": ORDER_TYPE_SELL if is_long else ORDER_TYPE_BUY,
        "position": int(ticket),
        "price": tick.bid if is_long else tick.ask,
        "deviation": deviation,
        "magic": int(magic),
        "comment": "close",
        "type_time": ORDER_TIME_GTC,
        "type_filling": filling_for(mt5, symbol),
    }
    return _send(mt5, request)


def modify_stop(
    mt5: Any,
    *,
    symbol: str,
    ticket: int,
    stop_loss: float,
    take_profit: float,
) -> dict:
    """Déplacer le stop d'une position ouverte.

    **La cible doit être transmise aussi.** `TRADE_ACTION_SLTP` remplace les
    deux valeurs d'un coup : omettre `tp` ne le laisse pas tel quel, il
    l'efface. Une remontée de stop qui supprimerait la cible transformerait
    une jambe destinée à sortir à 4 104 en position sans sortie — exactement
    l'inverse du service rendu.
    """
    request = {
        "action": TRADE_ACTION_SLTP,
        "symbol": symbol,
        "position": int(ticket),
        "sl": float(stop_loss),
        "tp": float(take_profit),
    }
    return _send(mt5, request)


def cancel_order(mt5: Any, ticket: int) -> dict:
    """Retirer un ordre en attente."""
    return _send(mt5, {"action": TRADE_ACTION_REMOVE, "order": int(ticket)})


def check(mt5: Any, request: dict) -> dict:
    """Soumettre une requête à la validation du serveur, sans l'exécuter.

    `order_check` interroge réellement le serveur du broker : il attrape ce
    qu'aucun calcul local ne peut voir — marge insuffisante au cours exact,
    symbole suspendu, distance de stop refusée à cet instant précis.
    """
    result = mt5.order_check(request)
    if result is None:
        raise OrderError(f"order_check sans réponse : {mt5.last_error()}")
    return {
        "retcode": result.retcode,
        "comment": result.comment,
        "margin": getattr(result, "margin", None),
        "margin_free": getattr(result, "margin_free", None),
        "balance": getattr(result, "balance", None),
    }


def _send(mt5: Any, request: dict) -> dict:
    result = mt5.order_send(request)
    if result is None:
        raise OrderError(f"order_send sans réponse : {mt5.last_error()}")

    payload = {
        "retcode": result.retcode,
        "comment": result.comment,
        "order": getattr(result, "order", 0),
        "deal": getattr(result, "deal", 0),
        "volume": getattr(result, "volume", 0.0),
        "price": getattr(result, "price", 0.0),
        "request_id": getattr(result, "request_id", 0),
    }
    if result.retcode not in (RETCODE_DONE, RETCODE_PLACED):
        raise OrderError(
            f"refusé par le broker : code {result.retcode}, {result.comment}"
        )
    return payload


def _horodatage_mt5(moment: datetime) -> int:
    """L'expiration telle que le paquet MetaTrader5 l'accepte : un entier.

    Deux erreurs se superposent ici, et la seconde ne se voit que si on
    corrige la première.

    D'abord le type. Le paquet refuse un `datetime` et rend `None` avec
    `(-2, 'Invalid "expiration" argument')` — un refus local, jamais transmis
    au broker. C'est ce qui a fait échouer les deux dépôts du 13/11.

    Ensuite l'échelle. MT5 compte des secondes depuis 1970 **lues dans
    l'horloge du serveur**, pas en UTC. L'heure murale du serveur doit donc
    être encodée comme si elle était UTC. Utiliser `.timestamp()` sur un
    `datetime` naïf ferait intervenir le fuseau de la VM, et ajouterait un
    décalage au décalage déjà appliqué.
    """
    return int(moment.replace(tzinfo=timezone.utc).timestamp())


def expiry_in_server_time(minutes: int, server_offset_seconds: int | None) -> datetime:
    """Date d'expiration, convertie dans l'horloge du serveur du broker.

    MT5 lit cette date dans son propre temps, pas en UTC. Avec le décalage de
    deux heures mesuré chez XM, une expiration posée naïvement en UTC
    annulerait l'ordre deux heures trop tôt — soit, pour une fenêtre de trente
    minutes, avant même qu'il ait existé.
    """
    if server_offset_seconds is None:
        raise OrderError(
            "décalage d'horloge non mesuré : impossible de dater une expiration. "
            "Le marché est probablement fermé."
        )
    moment = datetime.now(timezone.utc) + timedelta(minutes=minutes)
    return moment.replace(tzinfo=None) + timedelta(seconds=server_offset_seconds)
