"""One child process per MT5 terminal.

Why a process and not a thread: the `MetaTrader5` package keeps **one**
terminal connection per process. Calling `initialize()` with a second path
tears the first connection down, so serving several accounts from one process
would mean reconnecting between every call — slow, and it loses whatever the
terminal was doing. A process each keeps every connection warm, and a terminal
that hangs takes only its own worker with it.

The parent talks to a worker over a `multiprocessing` pipe with a plain
request/response protocol. Everything crossing the pipe is built from `dict`,
`list`, `str`, `int`, `float`, `bool` and `None`: MT5 returns namedtuples that
happen to pickle, but relying on that would couple the parent to the exact
package version running in the child.

Nothing here places, modifies or cancels an order. The execution layer is a
separate phase, and keeping this module provably read-only is the point: the
only MT5 calls it makes are `*_get`, `*_info` and `copy_rates_*`.
"""

from __future__ import annotations

import time
from datetime import datetime, timedelta, timezone
from multiprocessing.connection import Connection
from typing import Any

#: Grille d'arrondi de la mesure d'horloge : l'heure pleine.
#:
#: Le choix n'est pas cosmétique, il corrige une erreur. La mesure vaut
#: `décalage − ancienneté de la bougie`, donc un symbole qui ne cote pas tire
#: le résultat vers le bas. Arrondir au quart d'heure conservait fidèlement
#: cette erreur : une bougie vieille de trente minutes sur un serveur à UTC+2
#: donnait 5 400, soit UTC+1,5 — un décalage qui n'existe chez aucun broker,
#: et que rien ne signalait.
#:
#: À l'heure pleine, 5 400 remonte à 7 200 : l'arrondi rattrape jusqu'à une
#: demi-heure d'ancienneté, ce qui couvre largement le cas d'un indice avant
#: son ouverture pendant que l'or cote.
#:
#: Hypothèse assumée : le serveur du broker est à un nombre entier d'heures
#: d'UTC. C'est le cas de tous les brokers européens, XM compris. Un serveur
#: calé sur une demi-heure — l'Inde, par exemple — serait mal mesuré, et il
#: faudrait alors revenir à une grille plus fine *et* garantir la fraîcheur
#: autrement.
OFFSET_GRID_SECONDS = 3600

#: A measured offset outside this band is not an offset, it is a stale quote —
#: typically the Friday close read on a Sunday. No broker runs its server more
#: than half a day from UTC.
MAX_PLAUSIBLE_OFFSET_SECONDS = 12 * 3600

# MQL5's ENUM_DEAL_ENTRY and ENUM_DEAL_TYPE, spelled out rather than read off
# the module. Which constants the Python package re-exports has varied between
# releases, and an `AttributeError` here would take down history reading on an
# upgrade — for values fixed by the protocol, the literal is the safer source.
DEAL_ENTRY_IN = 0
DEAL_ENTRY_OUT = 1
DEAL_ENTRY_INOUT = 2
DEAL_ENTRY_OUT_BY = 3
DEAL_TYPE_BUY = 0
DEAL_TYPE_SELL = 1


class WorkerError(Exception):
    """A failure the parent should report rather than retry."""


# --- Entry point -------------------------------------------------------------


def run(
    conn: Connection,
    terminal: str,
    login: str,
    server: str,
    execution: dict | None = None,
) -> None:
    """Child process main loop. Never returns until the pipe closes.

    `execution` is `None` for a read-only account, and the module that knows
    how to send orders is then never imported. The capability does not exist
    in the process rather than existing and being refused — which is the
    difference between a door that is locked and a wall.
    """
    import MetaTrader5 as mt5  # imported in the child only

    if not mt5.initialize(path=terminal, portable=True):
        conn.send({"ok": False, "error": f"initialize a échoué : {mt5.last_error()}"})
        conn.close()
        return

    state = _State(mt5=mt5, login=login, server=server)

    if execution:
        try:
            _enable_execution(state, execution)
        except Exception as exc:  # noqa: BLE001 - reported, never swallowed
            conn.send({"ok": False, "error": f"exécution inutilisable : {exc}"})
            mt5.shutdown()
            conn.close()
            return

    try:
        # Confirm we landed on the account the configuration promised. A
        # portable terminal remembers its last session, so a copied folder can
        # silently be logged into the wrong account — and every trade pushed
        # from it would then be filed against the wrong journal account.
        info = mt5.account_info()
        if info is None:
            conn.send({"ok": False, "error": f"terminal non connecté : {mt5.last_error()}"})
            return
        if str(info.login) != login:
            conn.send({
                "ok": False,
                "error": (
                    f"le terminal {terminal} est connecté au compte {info.login}, "
                    f"la configuration annonce {login}"
                ),
            })
            return

        conn.send({"ok": True, "result": {"login": str(info.login), "server": info.server}})

        while True:
            try:
                message = conn.recv()
            except EOFError:
                return
            except KeyboardInterrupt:
                # Ctrl+C sur le parent interrompt l'attente ici. C'est un
                # arrêt voulu, pas une panne : sortir en silence vaut mieux
                # qu'une trace d'exception qui donnerait à croire le
                # contraire.
                return
            conn.send(_dispatch(state, message))
    finally:
        mt5.shutdown()
        conn.close()


class _State:
    """Per-worker state: the module handle and the cached clock offset."""

    def __init__(self, mt5: Any, login: str, server: str) -> None:
        self.mt5 = mt5
        self.login = login
        self.server = server
        #: Seconds to subtract from an MT5 timestamp to get real UTC.
        #: `None` until a measurement succeeds — see `_server_offset`.
        self.offset: int | None = None
        self.offset_measured_at: float | None = None
        #: `None` tant que l'exécution n'est pas activée pour ce compte.
        self.orders: Any = None
        self.audit: Any = None
        self.expiry_minutes: int = 30


def _enable_execution(state: _State, settings: dict) -> None:
    """Charger de quoi envoyer, et vérifier le journal d'audit tout de suite.

    L'ordre compte : le journal est ouvert ici, au démarrage, précisément pour
    qu'un disque plein ou un chemin faux se découvre maintenant et non au
    moment d'envoyer. `Audit` écrit une ligne à sa construction, donc une
    réussite ici prouve que le disque accepte des écritures.
    """
    import audit
    import orders

    state.audit = audit.Audit(settings["audit_dir"], settings["alias"])
    state.orders = orders
    state.expiry_minutes = int(settings.get("expiry_minutes", 30))
    _HANDLERS.update(_EXECUTION_HANDLERS)


def _dispatch(state: _State, message: dict) -> dict:
    method = message.get("method")
    params = message.get("params") or {}
    handler = _HANDLERS.get(method)
    if handler is None:
        return {"ok": False, "error": f"méthode inconnue : {method}"}
    try:
        return {"ok": True, "result": handler(state, **params)}
    except WorkerError as exc:
        return {"ok": False, "error": str(exc)}
    except Exception as exc:  # noqa: BLE001 - the pipe must never carry a traceback
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


# --- Clock -------------------------------------------------------------------


def _server_offset(state: _State) -> int | None:
    """Seconds between the terminal's clock and real UTC, or `None`.

    MT5 hands out every timestamp as a Unix epoch value that is really the
    *broker server's* local time. Treating one as UTC files a trade two or
    three hours off, which is enough to put an 08:15 London entry in the wrong
    session — and, at the edges of the day, on the wrong calendar date.

    The measurement uses the open time of the current M1 bar, which during
    market hours is at most sixty seconds old, and rounds to the offset grid.
    When the market is closed there is no fresh bar, so the last measurement is
    reused; if none was ever taken, callers are told the basis is server time
    rather than being handed a plausible-looking lie.
    """
    # Re-measure at most once a minute: this costs a terminal round trip and
    # the answer only moves twice a year.
    if state.offset is not None and state.offset_measured_at is not None:
        if time.time() - state.offset_measured_at < 60:
            return state.offset

    # Tous les candidats sont interrogés, et c'est **le plus grand écart** qui
    # gagne — pas le premier qui répond.
    #
    # L'écart mesuré vaut `décalage − ancienneté de la bougie`. Un symbole qui
    # ne cote pas encore rend sa dernière bougie connue, vieille de plusieurs
    # heures, et donne donc un écart trop petit. S'arrêter au premier qui
    # répond revenait à faire confiance à n'importe quel symbole, y compris un
    # indice avant son ouverture : le 09/11 à 8 h 52 à Paris, le GER40 a rendu
    # une bougie de la veille et la mesure est sortie à UTC+1,5 au lieu de
    # UTC+2. Le maximum est l'échantillon le moins périmé, et il suffit qu'un
    # seul symbole cote pour qu'il soit juste.
    mt5 = state.mt5
    meilleur = None
    for symbol in _candidate_symbols(state):
        rates = mt5.copy_rates_from_pos(symbol, mt5.TIMEFRAME_M1, 0, 1)
        if rates is None or len(rates) == 0:
            continue
        difference = int(rates[0]["time"]) - int(time.time())
        if abs(difference) > MAX_PLAUSIBLE_OFFSET_SECONDS:
            continue  # bougie trop vieille pour être un décalage
        if meilleur is None or difference > meilleur:
            meilleur = difference

    if meilleur is None:
        return state.offset

    state.offset = int(round(meilleur / OFFSET_GRID_SECONDS) * OFFSET_GRID_SECONDS)
    state.offset_measured_at = time.time()
    return state.offset


#: Plafond du nombre de symboles interrogés pour la mesure d'horloge. Assez
#: large pour qu'au moins un cote à toute heure — l'or et le forex tournent
#: quand les indices dorment — sans interroger tout le Market Watch à chaque
#: minute.
MAX_CLOCK_SYMBOLS = 12


def _candidate_symbols(state: _State) -> list[str]:
    """Symboles sur lesquels mesurer l'horloge.

    Tout ce qui est dans le Market Watch convient, et il en faut plusieurs :
    un seul suffirait si l'on pouvait garantir qu'il cote, or aucun ne cote
    vingt-quatre heures sur vingt-quatre. Les indices dorment la nuit, le
    forex ferme le week-end, l'or s'arrête une heure par jour. C'est la
    diversité de l'échantillon qui rend la mesure robuste, pas le choix d'un
    symbole privilégié.
    """
    symbols = state.mt5.symbols_get()
    if not symbols:
        return []
    selected = [s.name for s in symbols if s.visible]
    return selected[:MAX_CLOCK_SYMBOLS] or [symbols[0].name]


def _to_utc(state: _State, mt5_epoch: int | float | None) -> str | None:
    """An MT5 timestamp as an ISO-8601 UTC string the journal can store."""
    if not mt5_epoch:
        return None
    offset = _server_offset(state)
    if offset is None:
        offset = 0
    moment = datetime.fromtimestamp(int(mt5_epoch) - offset, tz=timezone.utc)
    return moment.strftime("%Y-%m-%dT%H:%M:%S")


def _to_server_datetime(state: _State, iso: str) -> datetime:
    """A UTC instant expressed in the terminal's own clock.

    `history_deals_get` reads its bounds in server time, so a range given in
    UTC must be shifted before it is handed over, or a query for "today" will
    miss the first hours of the session.
    """
    moment = datetime.fromisoformat(iso.replace("Z", "+00:00"))
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    offset = _server_offset(state) or 0
    return moment.astimezone(timezone.utc).replace(tzinfo=None) + timedelta(seconds=offset)


# --- Handlers ----------------------------------------------------------------


def _account(state: _State) -> dict:
    mt5 = state.mt5
    info = mt5.account_info()
    if info is None:
        raise WorkerError(f"compte illisible : {mt5.last_error()}")
    terminal = mt5.terminal_info()

    hedging = info.margin_mode == mt5.ACCOUNT_MARGIN_MODE_RETAIL_HEDGING
    offset = _server_offset(state)

    return {
        "login": str(info.login),
        "server": info.server,
        "company": info.company,
        "currency": info.currency,
        "balance": info.balance,
        "equity": info.equity,
        "profit": info.profit,
        "margin": info.margin,
        "margin_free": info.margin_free,
        "margin_level": info.margin_level,
        "leverage": info.leverage,
        # The whole partial-exit design depends on this being hedging: in
        # netting mode a second order on the same symbol merges into the
        # existing position instead of standing beside it.
        "margin_mode": "hedging" if hedging else "netting",
        # False means the terminal's Algo Trading button is off. Harmless
        # while we only read, fatal the day we send an order, so it is
        # surfaced from the start.
        "algo_trading_enabled": bool(terminal.trade_allowed) if terminal else None,
        "server_utc_offset_seconds": offset,
        "time_basis": "utc" if offset is not None else "server",
    }


def _positions(state: _State, symbol: str | None = None) -> list[dict]:
    mt5 = state.mt5
    raw = mt5.positions_get(symbol=symbol) if symbol else mt5.positions_get()
    if raw is None:
        raise WorkerError(f"positions illisibles : {mt5.last_error()}")

    return [
        {
            "ticket": str(p.ticket),
            "symbol": p.symbol,
            "direction": "LONG" if p.type == mt5.POSITION_TYPE_BUY else "SHORT",
            "volume": p.volume,
            "open_time": _to_utc(state, p.time),
            "open_price": p.price_open,
            "current_price": p.price_current,
            "stop_loss": p.sl or None,
            "take_profit": p.tp or None,
            "profit": p.profit,
            "swap": p.swap,
            "magic": p.magic,
            "comment": p.comment,
        }
        for p in raw
    ]


def _orders(state: _State, symbol: str | None = None) -> list[dict]:
    mt5 = state.mt5
    raw = mt5.orders_get(symbol=symbol) if symbol else mt5.orders_get()
    if raw is None:
        raise WorkerError(f"ordres illisibles : {mt5.last_error()}")

    return [
        {
            "ticket": str(o.ticket),
            "symbol": o.symbol,
            "type": _ORDER_TYPES.get(o.type, str(o.type)),
            "volume": o.volume_current,
            "price": o.price_open,
            "stop_loss": o.sl or None,
            "take_profit": o.tp or None,
            "placed_at": _to_utc(state, o.time_setup),
            "expires_at": _to_utc(state, o.time_expiration),
            "magic": o.magic,
            "comment": o.comment,
        }
        for o in raw
    ]


def _history(state: _State, date_from: str, date_to: str, symbol: str | None = None) -> list[dict]:
    """Closed trades over a UTC range, deals folded back into positions.

    MT5 stores *deals*, not trades: an entry and an exit are two rows, and a
    position closed in thirds is four. The journal wants one row per broker
    ticket, so deals are grouped by `position_id` and the volume-weighted
    entry and exit prices are rebuilt from them.
    """
    mt5 = state.mt5
    deals = mt5.history_deals_get(
        _to_server_datetime(state, date_from),
        _to_server_datetime(state, date_to),
    )
    if deals is None:
        raise WorkerError(f"historique illisible : {mt5.last_error()}")

    grouped: dict[int, list[Any]] = {}
    for deal in deals:
        # Deposits, credits, corrections and commissions booked on the account
        # rather than on a trade: no position, nothing to fold.
        if deal.position_id == 0 or deal.type not in (DEAL_TYPE_BUY, DEAL_TYPE_SELL):
            continue
        if symbol and deal.symbol != symbol:
            continue
        grouped.setdefault(deal.position_id, []).append(deal)

    trades = [_fold_position(state, position_id, deals) for position_id, deals in grouped.items()]
    trades.sort(key=lambda t: t["open_time"] or "")
    return trades


def _fold_position(state: _State, position_id: int, deals: list[Any]) -> dict:
    deals = sorted(deals, key=lambda d: (d.time_msc, d.ticket))

    # A reversal deal (`INOUT`) both closes and reopens; counting it as an
    # entry keeps the resulting position's volume right, which is what the
    # journal reads. It is not a shape this project produces — plans only ever
    # open and close — but a hand-flipped position in the terminal would
    # otherwise come back with no entry at all.
    entries = [d for d in deals if d.entry in (DEAL_ENTRY_IN, DEAL_ENTRY_INOUT)]
    exits = [d for d in deals if d.entry in (DEAL_ENTRY_OUT, DEAL_ENTRY_OUT_BY)]

    opened = entries[0] if entries else deals[0]
    direction = "LONG" if opened.type == DEAL_TYPE_BUY else "SHORT"

    return {
        "ticket": str(position_id),
        "symbol": opened.symbol,
        "direction": direction,
        "volume": round(sum(d.volume for d in entries), 8),
        "open_time": _to_utc(state, opened.time),
        "open_price": _weighted_price(entries),
        "close_time": _to_utc(state, exits[-1].time) if exits else None,
        "close_price": _weighted_price(exits),
        # Summed across every deal: a position closed in thirds pays its
        # commission three times, and a P&L that ignored that would overstate
        # the result of exactly the trades this project is built around.
        "profit": round(sum(d.profit for d in deals), 2),
        "commission": round(sum(d.commission for d in deals), 2),
        "swap": round(sum(d.swap for d in deals), 2),
        "fee": round(sum(getattr(d, "fee", 0.0) for d in deals), 2),
        "magic": opened.magic,
        "comment": opened.comment,
        "closed": bool(exits),
        "deals": len(deals),
    }


def _weighted_price(deals: list[Any]) -> float | None:
    """Volume-weighted average price, or `None` when there is nothing to weigh."""
    volume = sum(d.volume for d in deals)
    if volume <= 0:
        return None
    return round(sum(d.price * d.volume for d in deals) / volume, 8)


def _symbol(state: _State, symbol: str) -> dict:
    """Everything position sizing and order placement need about an instrument."""
    mt5 = state.mt5
    if not mt5.symbol_select(symbol, True):
        raise WorkerError(f"symbole inconnu du terminal : {symbol}")
    info = mt5.symbol_info(symbol)
    if info is None:
        raise WorkerError(f"spécifications illisibles pour {symbol} : {mt5.last_error()}")

    filling = [name for bit, name in _FILLING_MODES.items() if info.filling_mode & bit]

    return {
        "name": info.name,
        "description": info.description,
        "digits": info.digits,
        "point": info.point,
        "tick_size": info.trade_tick_size,
        "tick_value": info.trade_tick_value,
        "contract_size": info.trade_contract_size,
        "volume_min": info.volume_min,
        "volume_step": info.volume_step,
        "volume_max": info.volume_max,
        # Zero means the broker imposes no minimum distance *today*. Several
        # apply one dynamically around news, so a plan must still handle a
        # rejected stop rather than trust this.
        "stops_level": info.trade_stops_level,
        "freeze_level": info.trade_freeze_level,
        # A market order must declare a filling policy the symbol allows;
        # sending the wrong one is rejected outright rather than adjusted.
        "filling_modes": filling,
        "trade_mode": _TRADE_MODES.get(info.trade_mode, str(info.trade_mode)),
        "currency_profit": info.currency_profit,
        # The smallest position that can still be split into three legs.
        "min_volume_for_three_legs": round(info.volume_min * 3, 8),
    }


def _quote(state: _State, symbol: str) -> dict:
    mt5 = state.mt5
    if not mt5.symbol_select(symbol, True):
        raise WorkerError(f"symbole inconnu du terminal : {symbol}")
    tick = mt5.symbol_info_tick(symbol)
    if tick is None:
        raise WorkerError(f"aucune cotation pour {symbol} : {mt5.last_error()}")

    info = mt5.symbol_info(symbol)
    spread_points = None
    if info and info.point:
        spread_points = round((tick.ask - tick.bid) / info.point, 1)

    return {
        "symbol": symbol,
        "bid": tick.bid,
        "ask": tick.ask,
        "last": tick.last or None,
        "spread_points": spread_points,
        "time": _to_utc(state, tick.time),
        # A bid of zero on a freshly selected symbol is not a price: the
        # terminal has simply not received a tick yet, which is the normal
        # state at the weekend.
        "quoting": bool(tick.bid and tick.ask),
    }


def _market_watch(state: _State) -> list[dict]:
    mt5 = state.mt5
    symbols = mt5.symbols_get()
    if symbols is None:
        raise WorkerError(f"symboles illisibles : {mt5.last_error()}")
    return [
        {"name": s.name, "description": s.description}
        for s in symbols
        if s.visible
    ]


def _day_state(state: _State) -> dict:
    """Where the account stands today, for the guards.

    The broker is the source of truth here, not the journal: a trade taken by
    hand in the terminal counts against the daily allowance just as much as
    one the bridge placed, and the journal may not have heard about it yet.
    """
    mt5 = state.mt5
    now = datetime.now(timezone.utc)
    midnight = now.replace(hour=0, minute=0, second=0, microsecond=0)

    deals = mt5.history_deals_get(
        _to_server_datetime(state, midnight.strftime("%Y-%m-%dT%H:%M:%S")),
        _to_server_datetime(state, now.strftime("%Y-%m-%dT%H:%M:%S")),
    )
    if deals is None:
        raise WorkerError(f"historique du jour illisible : {mt5.last_error()}")

    # Entry deals carry a zero profit but a real commission, so summing over
    # all of them is right: the fee is paid whether or not the trade is closed.
    realised = sum(
        d.profit + d.commission + d.swap
        for d in deals
        if d.type in (DEAL_TYPE_BUY, DEAL_TYPE_SELL)
    )

    positions = mt5.positions_get() or ()
    # Les ordres en attente comptent autant que les positions. Un plan déposé
    # le matin réserve son créneau dès la pose : ne regarder que les positions
    # ouvertes le rendrait invisible au plafond de concurrence, et deux plans
    # pourraient se superposer alors qu'un seul est autorisé. C'est
    # exactement la réservation immédiate qu'on a choisie plutôt que de
    # revérifier au déclenchement.
    pending = mt5.orders_get() or ()

    # Un plan est identifié par son magic. Ce qui est ouvert à la main porte
    # zéro et compte pour un groupe à part : ça occupe le compte tout pareil.
    plan_magics = {p.magic for p in positions if p.magic} | {
        o.magic for o in pending if o.magic
    }
    manual = any(p.magic == 0 for p in positions) or any(o.magic == 0 for o in pending)

    return {
        "day": now.strftime("%Y-%m-%d"),
        "time_basis": "utc" if _server_offset(state) is not None else "server",
        "realised_pnl": round(realised, 2),
        "open_positions": len(positions),
        "pending_orders": len(pending),
        "open_plans": len(plan_magics) + (1 if manual else 0),
        "plan_magics": sorted(plan_magics),
        "has_manual_positions": manual,
        "floating_pnl": round(sum(p.profit for p in positions), 2),
    }


def _margin_for(state: _State, symbol: str, direction: str, volume: float) -> dict:
    """Margin a hypothetical order would lock up. Sends nothing.

    `order_calc_margin` is a pure calculation inside the terminal — it does
    not reach the server and cannot place anything.
    """
    mt5 = state.mt5
    if not mt5.symbol_select(symbol, True):
        raise WorkerError(f"symbole inconnu du terminal : {symbol}")
    tick = mt5.symbol_info_tick(symbol)
    if tick is None or not (tick.bid and tick.ask):
        raise WorkerError(f"aucune cotation pour {symbol}")

    is_long = direction.upper() in ("LONG", "BUY")
    order_type = mt5.ORDER_TYPE_BUY if is_long else mt5.ORDER_TYPE_SELL
    price = tick.ask if is_long else tick.bid

    margin = mt5.order_calc_margin(order_type, symbol, volume, price)
    if margin is None:
        raise WorkerError(f"marge incalculable : {mt5.last_error()}")

    return {"symbol": symbol, "volume": volume, "price": price, "margin": margin}


def _find_symbols(state: _State, query: str) -> list[dict]:
    """Chercher un instrument parmi **tous** ceux du broker, pas seulement
    ceux du Market Watch.

    Un symbole absent du Market Watch existe quand même et peut y être ajouté.
    Ne chercher que parmi les visibles ferait conclure à tort qu'un instrument
    n'est pas proposé — alors qu'il l'est, sous un nom qu'on n'avait pas
    deviné. Chez XM, tout porte un suffixe lié au type de compte.
    """
    mt5 = state.mt5
    motif = f"*{query.strip().upper()}*"
    trouves = mt5.symbols_get(motif) or ()
    return [
        {
            "name": s.name,
            "description": s.description,
            "visible": s.visible,
            "trade_mode": _TRADE_MODES.get(s.trade_mode, str(s.trade_mode)),
            "volume_min": s.volume_min,
            "volume_step": s.volume_step,
        }
        for s in trouves
    ]


def _clock(state: _State) -> dict:
    """Décalage d'horloge, et l'échantillon qui l'a produit.

    Les mesures par symbole sont rendues parce qu'elles sont le seul moyen de
    voir qu'une d'elles est périmée. Sans ça, un chiffre faux ressemble
    exactement à un chiffre juste — c'est ce qui s'est passé le 09/11, où le
    GER40 avant son ouverture a tiré la mesure à UTC+1,5.
    """
    offset = _server_offset(state)
    mt5 = state.mt5
    maintenant = int(time.time())

    echantillon = []
    for symbol in _candidate_symbols(state):
        rates = mt5.copy_rates_from_pos(symbol, mt5.TIMEFRAME_M1, 0, 1)
        if rates is None or len(rates) == 0:
            continue
        ecart = int(rates[0]["time"]) - maintenant
        echantillon.append({
            "symbol": symbol,
            "offset_seconds": ecart,
            # L'ancienneté se déduit une fois le décalage retenu : c'est
            # l'écart entre ce que ce symbole dit et ce que dit le plus frais.
            "stale_seconds": None if offset is None else offset - ecart,
        })

    return {
        "utc_now": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S"),
        "server_utc_offset_seconds": offset,
        "server_utc_offset_hours": None if offset is None else offset / 3600,
        "measured": offset is not None,
        "samples": sorted(echantillon, key=lambda s: -s["offset_seconds"]),
        "note": (
            "retenu : l'écart le plus grand de l'échantillon, donc le symbole "
            "le moins périmé"
            if offset is not None
            else "aucun symbole ne cote et aucune mesure en cache : les "
                 "horodatages sont en heure serveur"
        ),
    }


def _execute(
    state: _State,
    plan_id: str,
    symbol: str,
    direction: str,
    stop_loss: float,
    legs: list[dict],
    magic: int,
    pending_at: float | None = None,
) -> dict:
    """Envoyer les jambes d'un plan, au marché ou en attente.

    Les jambes partent **une par une et séquentiellement**, et l'envoi
    s'arrête à la première qui échoue. Rien n'est défait automatiquement :
    chaque jambe déjà passée porte son propre stop, donc elle est protégée, et
    tenter de la refermer dans la foulée ajouterait un envoi qui peut lui
    aussi échouer — on remplacerait un plan incomplet par une cascade. Ce qui
    est garanti, c'est que la réponse dit exactement quelles jambes sont
    ouvertes et laquelle a été refusée.
    """
    if state.orders is None or state.audit is None:
        raise WorkerError("exécution non activée pour ce compte")

    intention = {
        "symbol": symbol,
        "direction": direction,
        "stop_loss": stop_loss,
        "legs": legs,
        "magic": magic,
        "mode": "pending" if pending_at else "market",
        "pending_at": pending_at,
    }
    # Écrit avant le premier appel au broker. Si la machine meurt ici, le
    # fichier porte une intention sans résultat, et `orphans()` la retrouve.
    state.audit.write("order_intent", intention, plan_id=plan_id)

    expiration = None
    if pending_at:
        expiration = state.orders.expiry_in_server_time(
            state.expiry_minutes, _server_offset(state)
        )

    envoyees: list[dict] = []
    try:
        for index, leg in enumerate(legs, start=1):
            commun = dict(
                symbol=symbol,
                direction=direction,
                volume=float(leg["volume"]),
                stop_loss=stop_loss,
                take_profit=float(leg["take_profit"]),
                magic=magic,
                comment=f"{plan_id[:16]}#{index}",
            )
            if pending_at:
                resultat = state.orders.pending_leg(
                    state.mt5, entry=float(pending_at),
                    expires_at_server=expiration, **commun
                )
            else:
                resultat = state.orders.market_leg(state.mt5, **commun)
            envoyees.append({"leg": index, **resultat})
    except Exception as exc:  # noqa: BLE001 - toute erreur doit laisser une trace
        state.audit.write(
            "order_failed",
            {"sent": envoyees, "failed_leg": len(envoyees) + 1, "error": str(exc)},
            plan_id=plan_id,
        )
        raise WorkerError(
            f"jambe {len(envoyees) + 1} refusée : {exc}. "
            f"{len(envoyees)} jambe(s) déjà en place, chacune avec son stop."
        ) from exc

    resultat = {
        "plan_id": plan_id,
        "magic": magic,
        "mode": "pending" if pending_at else "market",
        "legs": envoyees,
        "expires_at_server": expiration.isoformat() if expiration else None,
    }
    state.audit.write("order_result", resultat, plan_id=plan_id)
    return resultat


def _cancel(state: _State, magic: int) -> dict:
    """Retirer les ordres en attente d'un plan, par son magic number."""
    if state.orders is None or state.audit is None:
        raise WorkerError("exécution non activée pour ce compte")

    mt5 = state.mt5
    en_attente = [o for o in (mt5.orders_get() or ()) if o.magic == magic]
    state.audit.write("cancel_intent", {"magic": magic, "tickets": [o.ticket for o in en_attente]})

    retires, echecs = [], []
    for ordre in en_attente:
        try:
            state.orders.cancel_order(mt5, ordre.ticket)
            retires.append(ordre.ticket)
        except Exception as exc:  # noqa: BLE001
            echecs.append({"ticket": ordre.ticket, "error": str(exc)})

    resultat = {"magic": magic, "cancelled": retires, "failed": echecs}
    state.audit.write("cancel_result", resultat)
    if echecs and not retires:
        raise WorkerError(f"aucun ordre retiré : {echecs}")
    return resultat


def _modify_stop(
    state: _State,
    ticket: str,
    symbol: str,
    stop_loss: float,
    take_profit: float,
) -> dict:
    """Remonter le stop d'une jambe, en conservant sa cible.

    Tracé au même titre qu'un envoi : déplacer un stop est une décision qui
    change le risque d'une position ouverte, et un journal d'audit qui ne
    retiendrait que les ouvertures raconterait l'histoire à moitié.
    """
    if state.orders is None or state.audit is None:
        raise WorkerError("exécution non activée pour ce compte")

    state.audit.write(
        "modify_intent",
        {"ticket": ticket, "symbol": symbol, "stop_loss": stop_loss,
         "take_profit": take_profit},
    )
    try:
        resultat = state.orders.modify_stop(
            state.mt5, symbol=symbol, ticket=int(ticket),
            stop_loss=float(stop_loss), take_profit=float(take_profit),
        )
    except Exception as exc:  # noqa: BLE001
        state.audit.write("modify_failed", {"ticket": ticket, "error": str(exc)})
        raise WorkerError(f"stop du ticket {ticket} non déplacé : {exc}") from exc

    state.audit.write("modify_result", {"ticket": ticket, **resultat})
    return resultat


def _close(
    state: _State,
    magic: int | None = None,
    ticket: str | None = None,
) -> dict:
    """Fermer une position, un plan entier, ou tout ce qui est ouvert.

    Sans `magic` ni `ticket`, ferme tout — y compris ce qui a été ouvert à la
    main dans le terminal. C'est assumé : « tout fermer » doit vouloir dire
    tout, sinon il reste une exposition que l'on croit soldée. L'appelant est
    chargé de faire confirmer.

    Les positions sont fermées une par une et l'échec de l'une n'arrête pas
    les autres. Réduire l'exposition est la direction sûre : s'interrompre à
    la première difficulté laisserait ouvert ce qui aurait pu être fermé.
    """
    if state.orders is None or state.audit is None:
        raise WorkerError("exécution non activée pour ce compte")

    mt5 = state.mt5
    positions = list(mt5.positions_get() or ())
    if magic is not None:
        positions = [p for p in positions if p.magic == int(magic)]
    if ticket is not None:
        positions = [p for p in positions if str(p.ticket) == str(ticket)]

    cible = {"magic": magic, "ticket": ticket, "count": len(positions),
             "tickets": [str(p.ticket) for p in positions]}
    state.audit.write("close_intent", cible)

    if not positions:
        state.audit.write("close_result", {**cible, "closed": [], "failed": []})
        return {"closed": [], "failed": [], "message": "aucune position correspondante"}

    fermees, echecs = [], []
    for position in positions:
        try:
            resultat = state.orders.close_position(
                mt5,
                symbol=position.symbol,
                ticket=position.ticket,
                volume=position.volume,
                direction="LONG" if position.type == mt5.POSITION_TYPE_BUY else "SHORT",
                magic=position.magic,
            )
            fermees.append({
                "ticket": str(position.ticket),
                "symbol": position.symbol,
                "volume": position.volume,
                "profit": position.profit,
                "price": resultat.get("price"),
            })
        except Exception as exc:  # noqa: BLE001
            echecs.append({"ticket": str(position.ticket), "error": str(exc)})

    resultat = {"closed": fermees, "failed": echecs,
                "realised": round(sum(f["profit"] for f in fermees), 2)}
    state.audit.write("close_result", {**cible, **resultat})
    return resultat


_EXECUTION_HANDLERS = {
    "execute": _execute,
    "cancel": _cancel,
    "modify_stop": _modify_stop,
    "close": _close,
}

_HANDLERS = {
    "account": _account,
    "positions": _positions,
    "orders": _orders,
    "history": _history,
    "symbol": _symbol,
    "quote": _quote,
    "market_watch": _market_watch,
    "find_symbols": _find_symbols,
    "clock": _clock,
    "day_state": _day_state,
    "margin_for": _margin_for,
}

_ORDER_TYPES = {
    2: "buy_limit",
    3: "sell_limit",
    4: "buy_stop",
    5: "sell_stop",
    6: "buy_stop_limit",
    7: "sell_stop_limit",
}

_FILLING_MODES = {1: "FOK", 2: "IOC", 4: "BOC"}

_TRADE_MODES = {
    0: "disabled",
    1: "long_only",
    2: "short_only",
    3: "close_only",
    4: "full",
}
