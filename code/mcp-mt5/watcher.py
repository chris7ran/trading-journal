"""Surveillance des plans ouverts : remontée des stops au point mort.

La décision est séparée de son exécution. `breakeven_moves` est une fonction
pure qui regarde l'état d'un plan et dit quels stops déplacer ; la boucle qui
interroge le terminal et envoie les modifications vient après. C'est ce qui
permet d'éprouver la règle sur des cas que le marché ne présentera pas deux
fois de suite.

**Ce que le watcher protège, et ce qu'il ne protège pas.** Les cibles et les
stops initiaux vivent chez le broker : ils s'exécutent même si cette machine
est éteinte. Ce que le watcher ajoute, c'est l'amélioration du stop après une
première sortie — donc son arrêt ne met jamais une position à nu, il la laisse
seulement avec le stop qu'elle avait au départ. C'est la raison pour laquelle
il peut tourner dans le même processus que le serveur sans que ce soit un
point de défaillance critique.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass

LONG = "LONG"
SHORT = "SHORT"

#: Intervalle entre deux tours. Les sorties partielles ne se jouent pas à la
#: seconde : ce qui compte est qu'un stop remonte dans la minute qui suit la
#: première cible, pas dans la seconde.
POLL_SECONDS = 20


@dataclass(frozen=True)
class OpenLeg:
    ticket: str
    volume: float
    open_price: float
    stop_loss: float | None
    take_profit: float | None


@dataclass(frozen=True)
class PlanState:
    """Ce qu'on sait d'un plan à un instant donné."""

    magic: int
    symbol: str
    direction: str
    open_legs: list[OpenLeg]
    #: Jambes déjà fermées en gain. Une seule suffit à déclencher la remontée.
    closed_in_profit: int
    #: Jambes fermées en perte : le stop initial a été touché.
    closed_at_loss: int

    @property
    def entry_price(self) -> float | None:
        """Prix d'entrée du plan, pondéré par le volume des jambes ouvertes.

        Calculé sur ce qui reste ouvert, et non sur l'ensemble des jambes :
        c'est le point mort des positions qu'on va modifier. Les trois jambes
        d'un même plan sont de toute façon remplies au même prix, à un cent
        près, donc la distinction est théorique — mais elle évite d'avoir
        besoin de l'historique pour décider.
        """
        volume = sum(leg.volume for leg in self.open_legs)
        if volume <= 0:
            return None
        return sum(leg.open_price * leg.volume for leg in self.open_legs) / volume


def breakeven_moves(
    state: PlanState,
    *,
    offset: float = 0.0,
) -> list[tuple[str, float]]:
    """Stops à déplacer, sous forme de couples (ticket, nouveau stop).

    La règle : dès qu'une jambe est sortie **en gain**, les jambes restantes
    voient leur stop remonter au prix d'entrée. Rien ne bouge avant, et rien
    ne bouge si la seule sortie a été une perte — le stop a alors fait son
    travail et il n'y a plus rien à protéger.

    Un stop n'est jamais **reculé**. La comparaison est explicite plutôt
    qu'implicite : sur un achat seul un stop plus haut est une amélioration,
    sur une vente c'est l'inverse, et une erreur de sens ici transformerait
    une protection en élargissement du risque.

    `offset` permet de poser le stop un peu au-delà de l'entrée. Zéro par
    défaut, mais zéro n'est pas tout à fait le point mort : sur un achat,
    l'entrée s'est faite à l'ask et le stop se déclenche sur le bid, donc une
    sortie à l'entrée exacte coûte encore le spread. Un offset de la taille du
    spread rend le point mort réel.
    """
    if state.closed_in_profit < 1:
        return []

    entree = state.entry_price
    if entree is None:
        return []

    cible = entree + offset if state.direction == LONG else entree - offset

    mouvements = []
    for leg in state.open_legs:
        actuel = leg.stop_loss
        if state.direction == LONG:
            ameliore = actuel is None or cible > actuel
        else:
            ameliore = actuel is None or cible < actuel
        if ameliore:
            mouvements.append((leg.ticket, round(cible, 5)))
    return mouvements


def group_by_plan(positions: list[dict], history: list[dict]) -> dict[int, PlanState]:
    """Reconstituer l'état de chaque plan à partir du terminal.

    Les positions ouvertes donnent les jambes restantes, l'historique du jour
    dit lesquelles sont déjà sorties et comment. Les positions sans magic —
    ouvertes à la main dans le terminal — sont ignorées : le watcher ne touche
    qu'à ce qu'il a posé lui-même.
    """
    plans: dict[int, dict] = {}

    for position in positions:
        magic = position.get("magic") or 0
        if not magic:
            continue
        entree = plans.setdefault(
            magic,
            {
                "symbol": position["symbol"],
                "direction": position["direction"],
                "legs": [],
                "profit": 0,
                "loss": 0,
            },
        )
        entree["legs"].append(
            OpenLeg(
                ticket=str(position["ticket"]),
                volume=float(position["volume"]),
                open_price=float(position["open_price"]),
                stop_loss=position.get("stop_loss"),
                take_profit=position.get("take_profit"),
            )
        )

    for trade in history:
        magic = trade.get("magic") or 0
        if not magic or magic not in plans or not trade.get("closed"):
            continue
        if float(trade.get("profit", 0)) > 0:
            plans[magic]["profit"] += 1
        elif float(trade.get("profit", 0)) < 0:
            plans[magic]["loss"] += 1

    return {
        magic: PlanState(
            magic=magic,
            symbol=data["symbol"],
            direction=data["direction"],
            open_legs=data["legs"],
            closed_in_profit=data["profit"],
            closed_at_loss=data["loss"],
        )
        for magic, data in plans.items()
    }


async def watch(terminal, *, offset: float = 0.0, interval: int = POLL_SECONDS) -> None:
    """Boucle de surveillance d'un compte. Tourne jusqu'à annulation.

    Toute erreur est consignée et la boucle continue. Un watcher qui s'arrête
    au premier accroc serait pire qu'absent : on le croirait en train de
    veiller.
    """
    alias = terminal.account.alias
    while True:
        try:
            await _tour(terminal, offset)
        except asyncio.CancelledError:
            raise
        except Exception as exc:  # noqa: BLE001
            print(f"[watcher {alias}] {type(exc).__name__}: {exc}", flush=True)
        await asyncio.sleep(interval)


async def _tour(terminal, offset: float) -> None:
    positions = (await terminal.call("positions"))
    if not positions:
        return

    # L'historique du jour suffit : un plan dont une jambe est sortie hier
    # n'a plus de stop à remonter aujourd'hui.
    from datetime import datetime, timedelta, timezone

    maintenant = datetime.now(timezone.utc)
    depuis = (maintenant - timedelta(days=1)).strftime("%Y-%m-%dT%H:%M:%S")
    history = await terminal.call(
        "history", date_from=depuis, date_to=maintenant.strftime("%Y-%m-%dT%H:%M:%S")
    )

    for state in group_by_plan(positions, history).values():
        # La cible doit repartir avec le stop : MT5 remplace les deux d'un
        # coup, et omettre la cible l'effacerait.
        cibles = {leg.ticket: leg.take_profit for leg in state.open_legs}
        for ticket, stop in breakeven_moves(state, offset=offset):
            await terminal.call(
                "modify_stop",
                ticket=ticket,
                symbol=state.symbol,
                stop_loss=stop,
                take_profit=cibles.get(ticket) or 0.0,
            )
            print(
                f"[watcher {terminal.account.alias}] plan {state.magic} : "
                f"stop du ticket {ticket} remonté à {stop}",
                flush=True,
            )
