"""Read-only MCP server over one or more MetaTrader 5 terminals.

Runs on the Windows VM that hosts the terminals, listens on the loopback
interface, and is published to the tailnet by `tailscale serve`. Nothing is
exposed to the internet and no port is opened on the router.

**This server cannot trade.** Every tool reads. Placing, modifying and closing
orders belongs to a later phase and to a different module, so that the day an
execution tool exists it is an obvious, reviewable addition rather than a line
buried in a file that already had order-sending code in it.

Architecture: the process you start owns no MT5 connection at all. It spawns
one child per configured terminal (`worker.py`) and routes tool calls to them
over pipes, because the `MetaTrader5` package allows a single terminal per
process. A child that dies is respawned on the next call.
"""

from __future__ import annotations

import argparse
import asyncio
import hmac
import multiprocessing as mp
import os
import secrets
import sys
import uuid
from datetime import datetime, timedelta, timezone
from multiprocessing.connection import Connection
from pathlib import Path
from typing import Any

import config
import guards
import journal
import plan
import watcher
import worker

#: Reported to clients in the initialize result. The SDK no longer substitutes
#: its own version when this is omitted, so an unset value would leave the
#: server anonymous in logs and client UIs.
SERVER_VERSION = "0.1.0"

#: A terminal can take a while to launch and log in on a cold VM.
START_TIMEOUT_SECONDS = 90

#: Ceiling on a single call. The terminal is single-threaded and a long
#: history query is genuinely slow, but a reply that never comes must surface
#: as an error rather than hang the MCP session.
CALL_TIMEOUT_SECONDS = 120

#: How far back `get_history` looks when no range is given.
DEFAULT_HISTORY_DAYS = 7


class ToolError(Exception):
    """A failure to report to the caller, in plain language."""


# --- One child process per terminal -----------------------------------------


class Terminal:
    """The parent's handle on one worker process."""

    def __init__(self, account: config.Account) -> None:
        self.account = account
        self._conn: Connection | None = None
        self._proc: mp.process.BaseProcess | None = None
        # Serialises calls: the pipe carries one request and one reply at a
        # time, so two concurrent tool calls would read each other's answers.
        self._lock = asyncio.Lock()

    @property
    def alive(self) -> bool:
        return self._proc is not None and self._proc.is_alive()

    async def call(self, method: str, **params: Any) -> Any:
        async with self._lock:
            # The pipe blocks, so it runs off the event loop; otherwise one
            # slow history query would freeze every other account's calls.
            return await asyncio.to_thread(self._call, method, params)

    def _call(self, method: str, params: dict) -> Any:
        self._start_if_needed()
        assert self._conn is not None
        try:
            self._conn.send({"method": method, "params": params})
            if not self._conn.poll(CALL_TIMEOUT_SECONDS):
                self._stop()
                raise ToolError(
                    f"{self.account.alias} : pas de réponse après "
                    f"{CALL_TIMEOUT_SECONDS} s, terminal relancé au prochain appel"
                )
            reply = self._conn.recv()
        except (EOFError, BrokenPipeError, OSError) as exc:
            self._stop()
            raise ToolError(f"{self.account.alias} : lien rompu avec le terminal ({exc})") from exc

        if not reply.get("ok"):
            raise ToolError(f"{self.account.alias} : {reply.get('error')}")
        return reply["result"]

    def _start_if_needed(self) -> None:
        if self.alive and self._conn is not None:
            return
        self._stop()

        parent_conn, child_conn = mp.Pipe()
        # L'exécution n'est transmise que si le compte l'a demandée. Sans ce
        # dictionnaire, le processus fils n'importe jamais le module d'envoi :
        # la capacité n'existe pas, au lieu d'exister et d'être refusée.
        execution = None
        if self.account.execution_enabled:
            execution = {
                "alias": self.account.alias,
                "audit_dir": str(self.account.audit_dir),
                "expiry_minutes": self.account.expiry_minutes,
            }

        proc = mp.Process(
            target=worker.run,
            args=(
                child_conn,
                str(self.account.terminal),
                self.account.login,
                self.account.server,
                execution,
            ),
            name=f"mt5-{self.account.alias}",
            daemon=True,
        )
        proc.start()
        child_conn.close()  # the parent must not hold the child's end open

        if not parent_conn.poll(START_TIMEOUT_SECONDS):
            proc.terminate()
            raise ToolError(
                f"{self.account.alias} : le terminal n'a pas répondu en "
                f"{START_TIMEOUT_SECONDS} s. Est-il lancé et connecté ?"
            )
        handshake = parent_conn.recv()
        if not handshake.get("ok"):
            proc.join(timeout=5)
            raise ToolError(f"{self.account.alias} : {handshake.get('error')}")

        self._conn, self._proc = parent_conn, proc

    def _stop(self) -> None:
        if self._conn is not None:
            try:
                self._conn.close()
            except OSError:
                pass
        if self._proc is not None and self._proc.is_alive():
            self._proc.terminate()
            self._proc.join(timeout=5)
        self._conn, self._proc = None, None


class Bridge:
    """The set of configured terminals, keyed by alias."""

    def __init__(self, accounts: list[config.Account]) -> None:
        self._terminals = {a.alias: Terminal(a) for a in accounts if a.enabled}
        self._disabled = [a.alias for a in accounts if not a.enabled]
        if not self._terminals:
            raise config.ConfigError("aucun compte activé dans accounts.toml")

    @property
    def aliases(self) -> list[str]:
        return list(self._terminals)

    @property
    def disabled(self) -> list[str]:
        return list(self._disabled)

    def get(self, alias: str) -> Terminal:
        terminal = self._terminals.get(alias.strip())
        if terminal is None:
            known = ", ".join(self._terminals) or "aucun"
            raise ToolError(f"compte inconnu : '{alias}'. Comptes configurés : {known}")
        return terminal

    def stop_all(self) -> None:
        for terminal in self._terminals.values():
            terminal._stop()  # noqa: SLF001 - shutdown path, same module's concern


# --- Tools -------------------------------------------------------------------


def register_tools(mcp: Any, bridge: Bridge) -> None:
    """Attach the read-only tool set to an `MCPServer` instance."""

    @mcp.tool()
    async def list_accounts() -> dict:
        """Lister les comptes MT5 configurés sur ce pont.

        À appeler en premier : toutes les autres fonctions prennent un alias
        de compte, et ce sont les seules valeurs acceptées. N'ouvre aucun
        terminal, donc répond instantanément même si MT5 n'est pas démarré.
        """
        return {
            "accounts": [
                {
                    "alias": t.account.alias,
                    "login": t.account.login,
                    "server": t.account.server,
                    "currency": t.account.currency,
                    "connected": t.alive,
                    "symbols": t.account.symbols,
                }
                for t in (bridge.get(a) for a in bridge.aliases)
            ],
            "disabled": bridge.disabled,
        }

    @mcp.tool()
    async def get_account(account: str) -> dict:
        """Solde, équité, marge, levier et mode de couverture d'un compte.

        `margin_mode` vaut `hedging` ou `netting` : en netting, un second
        ordre sur le même symbole fusionne avec la position existante au lieu
        de se placer à côté, ce qui invalide toute gestion par jambes.

        `time_basis` dit si les horodatages renvoyés par les autres fonctions
        sont en UTC réel ou en heure du serveur du broker. Hors séance, l'écart
        n'est pas mesurable et vaut `server`.
        """
        return await bridge.get(account).call("account")

    @mcp.tool()
    async def get_positions(account: str, symbol: str | None = None) -> dict:
        """Positions ouvertes, éventuellement filtrées sur un symbole.

        `symbol` accepte le nom du journal (`NAS100`) comme celui du broker
        (`US100Cash#`). Chaque position renvoyée porte les deux.
        """
        terminal = bridge.get(account)
        broker_symbol = terminal.account.to_broker(symbol) if symbol else None
        positions = await terminal.call("positions", symbol=broker_symbol)
        return {
            "account": account,
            "count": len(positions),
            "positions": [_with_journal_symbol(terminal, p) for p in positions],
        }

    @mcp.tool()
    async def get_pending_orders(account: str, symbol: str | None = None) -> dict:
        """Ordres en attente — limites et stops non encore déclenchés."""
        terminal = bridge.get(account)
        broker_symbol = terminal.account.to_broker(symbol) if symbol else None
        orders = await terminal.call("orders", symbol=broker_symbol)
        return {
            "account": account,
            "count": len(orders),
            "orders": [_with_journal_symbol(terminal, o) for o in orders],
        }

    @mcp.tool()
    async def get_history(
        account: str,
        date_from: str | None = None,
        date_to: str | None = None,
        symbol: str | None = None,
    ) -> dict:
        """Trades clôturés sur une période, un objet par position.

        Les dates sont en UTC, au format `YYYY-MM-DD` ou ISO-8601 complet.
        Sans dates, les sept derniers jours.

        MT5 ne stocke pas des trades mais des *deals* : une entrée et une
        sortie font deux lignes, et une position sortie en trois fois en fait
        quatre. Elles sont ici regroupées par position, avec des prix d'entrée
        et de sortie pondérés par le volume, et des frais additionnés sur
        l'ensemble des deals — une sortie en trois fois paie trois
        commissions.
        """
        terminal = bridge.get(account)
        start, end = _history_range(date_from, date_to)
        broker_symbol = terminal.account.to_broker(symbol) if symbol else None
        trades = await terminal.call(
            "history", date_from=start, date_to=end, symbol=broker_symbol
        )
        return {
            "account": account,
            "from": start,
            "to": end,
            "count": len(trades),
            "trades": [_with_journal_symbol(terminal, t) for t in trades],
        }

    @mcp.tool()
    async def get_symbol_spec(account: str, symbol: str) -> dict:
        """Spécifications d'un instrument : pas de volume, valeur du tick, modes d'exécution.

        C'est ce qui conditionne le dimensionnement. `volume_step` varie d'un
        instrument à l'autre chez un même broker — 0,1 sur les indices et 0,01
        sur l'or chez XM — donc il se lit, il ne se suppose pas.
        `min_volume_for_three_legs` donne la plus petite position encore
        découpable en trois.
        """
        terminal = bridge.get(account)
        spec = await terminal.call("symbol", symbol=terminal.account.to_broker(symbol))
        return _with_journal_symbol(terminal, spec, key="name")

    @mcp.tool()
    async def get_quote(account: str, symbol: str) -> dict:
        """Cotation courante : bid, ask et spread en points.

        `quoting` à faux signifie que le terminal n'a pas encore reçu de tick
        — l'état normal le week-end, à ne pas confondre avec un prix nul.
        """
        terminal = bridge.get(account)
        quote = await terminal.call("quote", symbol=terminal.account.to_broker(symbol))
        return _with_journal_symbol(terminal, quote)

    @mcp.tool()
    async def list_market_watch(account: str) -> dict:
        """Symboles présents dans le Market Watch du terminal.

        Un symbole absent ne peut être ni coté ni tradé tant qu'il n'y a pas
        été ajouté. Utile pour retrouver l'orthographe exacte d'un instrument,
        les brokers suffixant souvent leurs noms.
        """
        terminal = bridge.get(account)
        symbols = await terminal.call("market_watch")
        return {"account": account, "count": len(symbols), "symbols": symbols}

    @mcp.tool()
    async def preview_plan(
        account: str,
        symbol: str,
        direction: str,
        stop_loss: float,
        targets: list[float],
        risk_pct: float | None = None,
        entry: float | None = None,
        entry_low: float | None = None,
        entry_high: float | None = None,
    ) -> dict:
        """Chiffrer un plan de trade **sans rien envoyer**.

        Calcule la taille de position pour le risque voulu, la découpe en une
        jambe par cible, et passe le tout aux garde-fous. Aucun ordre n'est
        transmis : cet outil ne sait pas en envoyer.

        Le dimensionnement part du **prix de marché réel**, pas du prix écrit
        dans l'idée. Le stop, lui, n'a pas bougé : entrer dix points plus haut
        rend la perte réelle dix points plus grande que celle budgétée.

        `risk_pct` est une fraction — 0.005 pour un demi-pour-cent. Omis, il
        prend le plafond du compte. `targets` va de la cible la plus proche à
        la plus lointaine. `entry` ou le couple `entry_low`/`entry_high`
        servent à mesurer ce qu'un retard a coûté ; ils ne changent pas le
        prix d'exécution.

        Quand le compte déclare sa distribution d'issues, la réponse porte
        aussi `wait_instead` : la probabilité minimale de retour du prix
        au-delà de laquelle déposer un ordre en attente bat entrer maintenant.
        """
        return await _assess(
            bridge.get(account), account, symbol, direction, stop_loss, targets,
            risk_pct=risk_pct, entry=entry, entry_low=entry_low, entry_high=entry_high,
        )

    @mcp.tool()
    async def execute_plan(
        account: str,
        symbol: str,
        direction: str,
        stop_loss: float,
        targets: list[float],
        risk_pct: float | None = None,
        entry: float | None = None,
        confirm: bool = False,
    ) -> dict:
        """Envoyer le plan au marché, une jambe par cible.

        **Tout est recalculé ici**, au prix de l'instant : une
        prévisualisation d'il y a cinq minutes ne vaut rien sur un marché qui
        bouge, et exécuter des chiffres périmés serait précisément l'erreur
        que ce projet existe pour éviter.

        Un plan en zone orange est refusé tant que `confirm` n'est pas vrai.
        Un plan refusé par un garde-fou l'est quoi qu'il arrive : `confirm`
        lève une hésitation, pas un interdit.

        Chaque jambe part avec son stop et sa cible dans la même requête, donc
        les sorties vivent chez le broker et s'exécutent même si cette machine
        redémarre.
        """
        return await _send_plan(
            bridge.get(account), account, symbol, direction, stop_loss, targets,
            risk_pct=risk_pct, entry=entry, confirm=confirm, pending_at=None,
        )

    @mcp.tool()
    async def schedule_plan(
        account: str,
        symbol: str,
        direction: str,
        entry: float,
        stop_loss: float,
        targets: list[float],
        risk_pct: float | None = None,
        confirm: bool = False,
    ) -> dict:
        """Déposer le plan en attente au niveau voulu, et laisser le prix revenir.

        Utile quand le marché s'est éloigné : plutôt qu'entrer à un ratio
        dégradé ou renoncer, on attend le niveau prévu. Si le prix revient,
        l'entrée se fait au ratio complet ; sinon rien n'est perdu qu'une
        occasion.

        Le dimensionnement part de `entry`, et non du prix courant, puisque
        c'est là que l'ordre se remplira.

        Les ordres **expirent** après le délai configuré pour le compte. Sans
        péremption, un ordre oublié se déclencherait un jour sur un mouvement
        sans rapport avec la thèse d'origine.

        Le plan occupe un créneau et réserve son risque **dès la pose**, pas
        au déclenchement : sinon les garde-fous auraient été vérifiés contre
        un état périmé au moment où l'ordre se remplit.
        """
        return await _send_plan(
            bridge.get(account), account, symbol, direction, stop_loss, targets,
            risk_pct=risk_pct, entry=entry, confirm=confirm, pending_at=entry,
        )

    @mcp.tool()
    async def cancel_plan(account: str, magic: int) -> dict:
        """Retirer les ordres en attente d'un plan, par son `magic`.

        Rend le créneau avant l'expiration quand la thèse tombe. N'a aucun
        effet sur une position déjà ouverte — fermer une position est une
        autre décision, et ce pont ne sait pas le faire.
        """
        return await bridge.get(account).call("cancel", magic=int(magic))

    @mcp.tool()
    async def get_clock(account: str) -> dict:
        """Écart entre l'horloge du serveur du broker et l'UTC réel.

        MT5 renvoie chaque horodatage comme une époque Unix qui est en fait
        l'heure locale du serveur du broker. Prendre cette valeur pour de
        l'UTC décale un trade de deux ou trois heures, assez pour ranger une
        entrée de 08h15 dans la mauvaise session, et en bord de journée à la
        mauvaise date. La mesure n'est possible qu'en séance.
        """
        return await bridge.get(account).call("clock")


async def _assess(
    terminal: Terminal,
    account: str,
    symbol: str,
    direction: str,
    stop_loss: float,
    targets: list[float],
    *,
    risk_pct: float | None = None,
    entry: float | None = None,
    entry_low: float | None = None,
    entry_high: float | None = None,
    pending_at: float | None = None,
    _detail: bool = False,
):
    """Chiffrer un plan et le soumettre aux garde-fous.

    Partagé par la prévisualisation et les deux chemins d'envoi, pour que ce
    qui est exécuté soit littéralement ce qui a été jugé. Deux fonctions
    séparées finiraient par diverger, et c'est l'envoi qui aurait raison.
    """
    compte = terminal.account
    limits = guards.Limits(
        risk_pct_max=compte.risk_pct_max,
        daily_loss_pct_max=compte.daily_loss_pct_max,
        max_open_plans=compte.max_open_plans,
    )
    broker_symbol = compte.to_broker(symbol)

    info = await terminal.call("account")
    spec_raw = await terminal.call("symbol", symbol=broker_symbol)
    quote = await terminal.call("quote", symbol=broker_symbol)
    day = await terminal.call("day_state")

    spec = plan.SymbolSpec(
        name=spec_raw["name"], digits=spec_raw["digits"], point=spec_raw["point"],
        tick_size=spec_raw["tick_size"], tick_value=spec_raw["tick_value"],
        contract_size=spec_raw["contract_size"], volume_min=spec_raw["volume_min"],
        volume_step=spec_raw["volume_step"], volume_max=spec_raw["volume_max"],
        stops_level=spec_raw["stops_level"],
    )
    seuils = plan.Thresholds(green=compte.r_green, red=compte.r_red)
    is_long = str(direction).strip().upper() in ("LONG", "BUY", "ACHAT")
    market_price = (quote["ask"] if is_long else quote["bid"]) or 0.0

    # Un ordre en attente se remplira à son niveau, pas au prix courant : il
    # se dimensionne donc là-bas. Au marché, c'est l'inverse — seul le prix
    # réel compte.
    sizing_price = pending_at if pending_at else market_price

    cibles = [float(t) for t in targets]
    commun = dict(
        spec=spec, direction=direction, stop_loss=float(stop_loss), targets=cibles,
        equity=info["equity"],
        risk_pct=risk_pct if risk_pct is not None else limits.risk_pct_max,
        thresholds=seuils,
    )
    try:
        sized = plan.build(
            market_price=sizing_price, entry=entry,
            entry_range=(entry_low, entry_high)
            if entry_low is not None and entry_high is not None else None,
            **commun,
        )
    except plan.PlanError as exc:
        refus = {
            "account": account, "decision": "refusé",
            "reason": str(exc), "quote": quote,
        }
        return (refus, None, None) if _detail else refus

    margin = None
    try:
        margin = (await terminal.call(
            "margin_for", symbol=broker_symbol,
            direction=sized.direction, volume=sized.total_volume,
        ))["margin"]
    except ToolError:
        pass

    verdict = guards.check(
        sized=sized, limits=limits,
        state=guards.AccountState(
            equity=info["equity"], balance=info["balance"], currency=info["currency"],
            margin_free=info["margin_free"], today_realised_pnl=day["realised_pnl"],
            open_plans=day["open_plans"], open_positions=day["open_positions"],
            algo_trading_enabled=bool(info.get("algo_trading_enabled", True)),
            trade_mode=spec_raw["trade_mode"], quoting=bool(quote["quoting"]),
        ),
        margin_required=margin,
    )

    payload = {
        "account": account,
        "decision": verdict.decision,
        "findings": [
            {"level": f.level, "code": f.code, "message": f.message}
            for f in verdict.findings
        ],
        "plan": {
            "symbol": sized.symbol,
            "journal_symbol": compte.to_journal(sized.symbol),
            "direction": sized.direction,
            "reference_price": sized.reference_price,
            "stop_loss": sized.stop_loss,
            "stop_distance": round(sized.stop_distance, spec.digits),
            "total_volume": sized.total_volume,
            "risk_amount": sized.risk_amount,
            "risk_pct": round(sized.risk_pct, 5),
            "currency": info["currency"],
            "value_per_point": sized.value_per_point,
            "blended_r": sized.blended_r,
            "planned_blended_r": sized.planned_blended_r,
            "degradation": None if sized.degradation is None else round(sized.degradation, 3),
            "zone": sized.zone,
            "legs": [
                {"index": l.index, "volume": l.volume,
                 "take_profit": l.take_profit, "r_multiple": l.r_multiple}
                for l in sized.legs
            ],
            "margin_required": margin,
        },
        "account_state": {
            "equity": info["equity"], "balance": info["balance"],
            "margin_free": info["margin_free"], "realised_today": day["realised_pnl"],
            "open_plans": day["open_plans"], "open_positions": day["open_positions"],
            "pending_orders": day.get("pending_orders", 0),
            "daily_allowance": round(
                (info["balance"] - day["realised_pnl"]) * limits.daily_loss_pct_max, 2),
        },
        "limits": {
            "risk_pct_max": limits.risk_pct_max,
            "daily_loss_pct_max": limits.daily_loss_pct_max,
            "max_open_plans": limits.max_open_plans,
            "r_green": compte.r_green, "r_red": compte.r_red,
        },
    }

    attente = _wait_instead(compte, sized, commun, entry, pending_at)
    if attente:
        payload["wait_instead"] = attente

    return (payload, sized, verdict) if _detail else payload


def _wait_instead(compte, sized, commun: dict, entry: float | None, pending_at) -> dict | None:
    """Entrer maintenant, ou déposer un ordre au niveau prévu ?

    Rend la **probabilité minimale de retour du prix** au-delà de laquelle
    attendre rapporte davantage. Pas une recommandation : un seuil, que la
    mesure sur l'historique viendra confronter.

    Ne s'affiche que si le compte déclare sa distribution d'issues et que le
    plan a trois cibles, puisque la distribution décrit ce gabarit-là. Sans
    ces conditions, la comparaison serait bâtie sur une distribution supposée
    — et aurait exactement l'apparence d'une vraie.
    """
    if pending_at or entry is None or compte.outcomes is None:
        return None
    if len(sized.legs) != 3:
        return None
    if abs(entry - sized.reference_price) < 1e-9:
        return None

    issues = plan.Outcomes(reached=compte.outcomes, stop=compte.outcome_stop or 0.0)
    try:
        au_niveau = plan.build(market_price=entry, **commun)
    except plan.PlanError:
        return None

    ev_maintenant = issues.expectancy(sized.legs, sized.total_volume)
    ev_rempli = issues.expectancy(au_niveau.legs, au_niveau.total_volume)

    if ev_maintenant <= 0:
        seuil = 0.0
        lecture = "entrer maintenant a une espérance négative : attendre gagne quelle que soit la probabilité de retour"
    elif ev_rempli <= ev_maintenant:
        return None
    else:
        seuil = ev_maintenant / ev_rempli
        lecture = f"attendre bat entrer si le prix revient à {entry} avec au moins {seuil:.1%} de chances"

    return {
        "entry": entry,
        "expectancy_now_r": round(ev_maintenant, 4),
        "expectancy_if_filled_r": round(ev_rempli, 4),
        "breakeven_fill_probability": round(seuil, 4),
        "reading": lecture,
    }


async def _send_plan(
    terminal: Terminal,
    account: str,
    symbol: str,
    direction: str,
    stop_loss: float,
    targets: list[float],
    *,
    risk_pct: float | None,
    entry: float | None,
    confirm: bool,
    pending_at: float | None,
) -> dict:
    """Recalculer, juger, puis envoyer — jamais envoyer sur un jugement ancien."""
    if not terminal.account.execution_enabled:
        return {
            "account": account, "decision": "refusé",
            "reason": "l'exécution n'est pas activée pour ce compte "
                      "(execution.enabled dans accounts.toml)",
        }

    payload, sized, verdict = await _assess(
        terminal, account, symbol, direction, stop_loss, targets,
        risk_pct=risk_pct, entry=entry, pending_at=pending_at, _detail=True,
    )
    if sized is None or verdict is None:
        return payload

    if verdict.blocked:
        payload["sent"] = False
        return payload
    if verdict.needs_confirmation and not confirm:
        payload["sent"] = False
        payload["decision"] = "à confirmer"
        payload["hint"] = (
            "relancez avec confirm=true après avoir lu les avertissements ; "
            "confirm ne lève pas un refus, seulement une hésitation"
        )
        return payload

    plan_id = str(uuid.uuid4())
    # Le magic identifie le plan côté broker. Tiré au hasard sur 31 bits
    # plutôt que dérivé du plan_id : un UUID n'entre pas dans un entier, et
    # tronquer un hachage créerait des collisions silencieuses entre plans.
    magic = secrets.randbelow(2_000_000_000) + 1

    envoi = await terminal.call(
        "execute",
        plan_id=plan_id,
        symbol=sized.symbol,
        direction=sized.direction,
        stop_loss=sized.stop_loss,
        legs=[{"volume": l.volume, "take_profit": l.take_profit} for l in sized.legs],
        magic=magic,
        pending_at=pending_at,
    )

    payload["sent"] = True
    payload["execution"] = envoi
    payload["decision"] = "envoyé"
    return payload


def _with_journal_symbol(terminal: Terminal, payload: dict, key: str = "symbol") -> dict:
    """Add the journal's own name for the broker symbol in `payload[key]`."""
    broker_name = payload.get(key)
    if isinstance(broker_name, str):
        payload = {**payload, "journal_symbol": terminal.account.to_journal(broker_name)}
    return payload


def _history_range(date_from: str | None, date_to: str | None) -> tuple[str, str]:
    """Normalise a requested range to a pair of ISO-8601 UTC instants.

    A bare `YYYY-MM-DD` as the upper bound is widened to the end of that day:
    asking for history "to 6 November" and silently getting nothing after
    midnight would be the kind of off-by-one nobody notices until a trade is
    missing from a review.
    """
    now = datetime.now(timezone.utc)
    start = _parse_bound(date_from) if date_from else now - timedelta(days=DEFAULT_HISTORY_DAYS)
    end = _parse_bound(date_to, end_of_day=True) if date_to else now

    if end < start:
        raise ToolError("date_to est antérieure à date_from")
    return (
        start.strftime("%Y-%m-%dT%H:%M:%S"),
        end.strftime("%Y-%m-%dT%H:%M:%S"),
    )


def _parse_bound(value: str, end_of_day: bool = False) -> datetime:
    text = value.strip().replace("Z", "+00:00")
    try:
        moment = datetime.fromisoformat(text)
    except ValueError as exc:
        raise ToolError(f"date illisible : '{value}' (attendu YYYY-MM-DD ou ISO-8601)") from exc
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    if end_of_day and len(text) == 10:
        moment = moment + timedelta(days=1) - timedelta(seconds=1)
    return moment.astimezone(timezone.utc)


# --- Transport ---------------------------------------------------------------


class RequireToken:
    """ASGI guard: every request must carry the shared bearer token.

    Defence in depth rather than the main lock. The server binds to loopback
    and reaches the tailnet only through `tailscale serve`, so the tailnet's
    own device authentication is what actually gates access. This catches the
    case that layer cannot: another process on the same Windows VM.
    """

    def __init__(self, app: Any, token: str) -> None:
        self._app = app
        self._expected = f"Bearer {token}".encode()

    async def __call__(self, scope: dict, receive: Any, send: Any) -> None:
        if scope.get("type") != "http":
            await self._app(scope, receive, send)
            return

        provided = b""
        for name, value in scope.get("headers") or []:
            if name == b"authorization":
                provided = value
                break

        if not hmac.compare_digest(provided, self._expected):
            await send({
                "type": "http.response.start",
                "status": 401,
                "headers": [(b"content-type", b"application/json")],
            })
            await send({"type": "http.response.body", "body": b'{"error":"unauthorized"}'})
            return

        await self._app(scope, receive, send)


def transport_security(extra_hosts: list[str], port: int) -> Any:
    """DNS rebinding protection, widened to the names we are published under.

    The SDK rejects any request whose `Host` header it does not recognise,
    answering `421 Invalid Host header`. The default list is loopback only,
    which is right for a server reached directly — but `tailscale serve`
    forwards the browser's original host, so the tailnet name has to be
    declared or every proxied call is refused.

    The names stay out of the repository: this takes them from `--allowed-host`
    or `MCP_MT5_ALLOWED_HOSTS`, because a machine name and a tailnet are
    exactly the kind of detail a public repository should not carry.

    Turning the protection off entirely would be the lazy fix and a bad one:
    it is what stops a web page the user happens to visit from aiming requests
    at this server through their own browser.
    """
    from mcp.server.transport_security import TransportSecuritySettings

    # The Host header carries the port whenever it is not the scheme default,
    # so both spellings are registered.
    hosts = ["127.0.0.1", "localhost", f"127.0.0.1:{port}", f"localhost:{port}"]
    for host in extra_hosts:
        host = host.strip()
        if host and host not in hosts:
            hosts.append(host)

    return TransportSecuritySettings(
        enable_dns_rebinding_protection=True,
        allowed_hosts=hosts,
        # An absent Origin passes; these cover a client that sends one.
        allowed_origins=[f"https://{h}" for h in hosts] + [f"http://{h}" for h in hosts],
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Pont MCP en lecture seule vers MetaTrader 5")
    parser.add_argument("--config", type=Path, default=None, help="chemin de accounts.toml")
    parser.add_argument("--host", default="127.0.0.1", help="interface d'écoute")
    parser.add_argument("--port", type=int, default=8765, help="port d'écoute")
    parser.add_argument(
        "--allowed-host",
        action="append",
        default=[],
        metavar="HOTE[:PORT]",
        help=(
            "nom public accepté dans l'en-tête Host, répétable. "
            "Indispensable derrière tailscale serve, qui transmet le nom "
            "d'origine. Complète MCP_MT5_ALLOWED_HOSTS."
        ),
    )
    args = parser.parse_args()

    token = os.environ.get("MCP_MT5_TOKEN", "").strip()
    if not token:
        print(
            "MCP_MT5_TOKEN n'est pas défini.\n"
            "Générez-en un et exportez-le avant de démarrer :\n"
            '  $env:MCP_MT5_TOKEN = [guid]::NewGuid().ToString("N")',
            file=sys.stderr,
        )
        return 2

    if args.host not in ("127.0.0.1", "localhost", "::1"):
        # Binding elsewhere would put the order-free but account-revealing
        # surface on the LAN. `tailscale serve` proxies from loopback, so
        # there is never a reason to.
        print(
            f"refus d'écouter sur {args.host} : ce service reste en loopback, "
            "c'est tailscale serve qui le publie.",
            file=sys.stderr,
        )
        return 2

    try:
        accounts = config.load(args.config)
        bridge = Bridge(accounts)
    except config.ConfigError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    # Imported late so a configuration mistake is reported before the SDK is
    # even loaded — a missing terminal path should not be preceded by an
    # import traceback.
    #
    # SDK 2.x: `FastMCP` was renamed `MCPServer`, and the transport parameters
    # (`host`, `port`, …) moved off the constructor onto `run()` and the app
    # factories. The tool decorators are unchanged.
    from mcp.server.mcpserver import MCPServer

    mcp = MCPServer(
        "trading-journal-mt5",
        instructions=(
            "Pont en lecture seule vers un ou plusieurs terminaux MetaTrader 5. "
            "Appelez list_accounts en premier : toutes les autres fonctions "
            "prennent un alias de compte issu de cette liste. Aucun outil ne "
            "passe, ne modifie ni n'annule d'ordre."
        ),
        version=SERVER_VERSION,
    )
    register_tools(mcp, bridge)

    # Wrapped, not mounted: the token guard forwards every non-HTTP scope
    # untouched, so the MCP app's own lifespan still runs under uvicorn and
    # starts its session manager. Mounting it inside a Starlette app would
    # swallow that lifespan and the server would accept connections it could
    # never answer.
    extra_hosts = [
        *args.allowed_host,
        *[h for h in os.environ.get("MCP_MT5_ALLOWED_HOSTS", "").split(",") if h.strip()],
    ]
    app = RequireToken(
        mcp.streamable_http_app(transport_security=transport_security(extra_hosts, args.port)),
        token,
    )
    print(
        f"pont MCP MT5 sur http://{args.host}:{args.port}/mcp — "
        f"comptes : {', '.join(bridge.aliases)}",
        file=sys.stderr,
    )
    if extra_hosts:
        print(f"hôtes publics acceptés : {', '.join(extra_hosts)}", file=sys.stderr)
    try:
        asyncio.run(_serve(app, bridge, args))
    finally:
        bridge.stop_all()
    return 0


async def _serve(app: Any, bridge: Bridge, args) -> None:
    """Servir, avec un watcher par compte qui exécute.

    Le watcher tourne dans ce processus et partage les mêmes poignées de
    terminal, donc le verrou par compte sérialise naturellement ses appels
    avec ceux des outils — pas de requête concurrente sur un tube qui n'en
    accepte qu'une.

    Ce n'est pas un point de défaillance critique : les cibles et les stops
    initiaux vivent chez le broker. Si ce processus meurt, les sorties partent
    quand même, et ce qu'on perd est seulement l'amélioration du stop après
    une première sortie.
    """
    import uvicorn

    serveur = uvicorn.Server(
        uvicorn.Config(app, host=args.host, port=args.port, log_level="info")
    )

    # Le journal est optionnel et vit dans l'environnement, jamais dans le
    # dépôt : son adresse révèle le tailnet et son jeton ouvre une écriture.
    client = None
    url = os.environ.get("JOURNAL_URL", "").strip()
    jeton = os.environ.get("JOURNAL_INGEST_TOKEN", "").strip()
    if url and jeton:
        client = journal.Journal(url, jeton)
        print(f"remontée vers le journal : {url}", file=sys.stderr)
    elif url or jeton:
        # Une moitié de configuration est une erreur silencieuse : le watcher
        # tournerait sans rien remonter, et on le croirait en train de le
        # faire.
        print(
            "JOURNAL_URL et JOURNAL_INGEST_TOKEN vont ensemble : "
            "remontée désactivée",
            file=sys.stderr,
        )

    surveillances = [
        asyncio.create_task(
            watcher.watch(bridge.get(alias), journal=client), name=f"watcher-{alias}"
        )
        for alias in bridge.aliases
        if bridge.get(alias).account.execution_enabled
    ]
    if surveillances:
        print(
            f"watcher actif sur : {', '.join(t.get_name()[8:] for t in surveillances)}",
            file=sys.stderr,
        )

    try:
        await serveur.serve()
    finally:
        for tache in surveillances:
            tache.cancel()
        await asyncio.gather(*surveillances, return_exceptions=True)


if __name__ == "__main__":
    # Windows spawns child processes by re-importing this module, so every
    # start-up side effect has to stay behind this guard or each worker would
    # try to start its own server.
    mp.freeze_support()
    raise SystemExit(main())
