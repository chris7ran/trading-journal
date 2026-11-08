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

#: Broker server clocks sit at whole- or half-hour offsets from UTC. Rounding
#: a measurement to this grid absorbs the few seconds of latency between
#: asking the terminal for the time and reading our own clock.
OFFSET_GRID_SECONDS = 900

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


def run(conn: Connection, terminal: str, login: str, server: str) -> None:
    """Child process main loop. Never returns until the pipe closes."""
    import MetaTrader5 as mt5  # imported in the child only

    if not mt5.initialize(path=terminal, portable=True):
        conn.send({"ok": False, "error": f"initialize a échoué : {mt5.last_error()}"})
        conn.close()
        return

    state = _State(mt5=mt5, login=login, server=server)

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

    mt5 = state.mt5
    for symbol in _candidate_symbols(state):
        rates = mt5.copy_rates_from_pos(symbol, mt5.TIMEFRAME_M1, 0, 1)
        if rates is None or len(rates) == 0:
            continue
        difference = int(rates[0]["time"]) - int(time.time())
        if abs(difference) > MAX_PLAUSIBLE_OFFSET_SECONDS:
            continue  # stale bar, not an offset
        rounded = round(difference / OFFSET_GRID_SECONDS) * OFFSET_GRID_SECONDS
        state.offset = int(rounded)
        state.offset_measured_at = time.time()
        return state.offset

    return state.offset


def _candidate_symbols(state: _State) -> list[str]:
    """Symbols to measure the clock against, most liquid first.

    Anything in Market Watch will do; the ones already selected are tried
    first because they are the instruments actually being traded, so they are
    the ones quoting when it matters.
    """
    symbols = state.mt5.symbols_get()
    if not symbols:
        return []
    selected = [s.name for s in symbols if s.visible]
    return selected[:5] or [symbols[0].name]


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
    # A plan is identified by its magic number. Positions opened by hand carry
    # zero and count as one group between them — they occupy the account just
    # the same, so the concurrency ceiling has to see them.
    plan_magics = {p.magic for p in positions if p.magic}
    manual = any(p.magic == 0 for p in positions)

    return {
        "day": now.strftime("%Y-%m-%d"),
        "time_basis": "utc" if _server_offset(state) is not None else "server",
        "realised_pnl": round(realised, 2),
        "open_positions": len(positions),
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


def _clock(state: _State) -> dict:
    offset = _server_offset(state)
    return {
        "utc_now": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S"),
        "server_utc_offset_seconds": offset,
        "server_utc_offset_hours": None if offset is None else offset / 3600,
        "measured": offset is not None,
        "note": (
            "mesuré sur l'ouverture de la bougie M1 en cours"
            if offset is not None
            else "marché fermé et aucune mesure en cache : les horodatages sont en heure serveur"
        ),
    }


_HANDLERS = {
    "account": _account,
    "positions": _positions,
    "orders": _orders,
    "history": _history,
    "symbol": _symbol,
    "quote": _quote,
    "market_watch": _market_watch,
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
