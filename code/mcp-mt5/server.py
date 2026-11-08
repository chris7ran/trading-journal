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
import sys
from datetime import datetime, timedelta, timezone
from multiprocessing.connection import Connection
from pathlib import Path
from typing import Any

import config
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
        proc = mp.Process(
            target=worker.run,
            args=(
                child_conn,
                str(self.account.terminal),
                self.account.login,
                self.account.server,
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
    async def get_clock(account: str) -> dict:
        """Écart entre l'horloge du serveur du broker et l'UTC réel.

        MT5 renvoie chaque horodatage comme une époque Unix qui est en fait
        l'heure locale du serveur du broker. Prendre cette valeur pour de
        l'UTC décale un trade de deux ou trois heures, assez pour ranger une
        entrée de 08h15 dans la mauvaise session, et en bord de journée à la
        mauvaise date. La mesure n'est possible qu'en séance.
        """
        return await bridge.get(account).call("clock")


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

    import uvicorn

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
        uvicorn.run(app, host=args.host, port=args.port, log_level="info")
    finally:
        bridge.stop_all()
    return 0


if __name__ == "__main__":
    # Windows spawns child processes by re-importing this module, so every
    # start-up side effect has to stay behind this guard or each worker would
    # try to start its own server.
    mp.freeze_support()
    raise SystemExit(main())
