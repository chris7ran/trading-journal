"""Reading `accounts.toml`.

The real file never enters the repository: it names a broker, a live account
number and the path of a terminal on one specific machine. What is committed
is `accounts.example.toml`, with placeholders, and `.gitignore` keeps the real
one out.

One entry per MT5 terminal. Each gets its own child process at run time
because the `MetaTrader5` package holds a single terminal connection per
process — see `worker.py`.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

#: Where the config lives unless `--config` says otherwise: next to the code,
#: so the service has no dependency on the working directory it was started
#: from (Windows services start in `C:\Windows\System32`).
DEFAULT_CONFIG = Path(__file__).with_name("accounts.toml")


class ConfigError(Exception):
    """Raised with a message meant to be read by a person, not parsed."""


#: Ceilings applied when `accounts.toml` names none. Deliberately strict:
#: a forgotten setting must fail safe, not open the throttle.
DEFAULT_RISK_PCT_MAX = 0.01
DEFAULT_DAILY_LOSS_PCT_MAX = 0.02
DEFAULT_MAX_OPEN_PLANS = 1
DEFAULT_R_GREEN = 0.80
DEFAULT_R_RED = 0.50


@dataclass(frozen=True)
class Account:
    """One MT5 terminal the bridge can talk to."""

    alias: str
    login: str
    server: str
    terminal: Path
    currency: str
    enabled: bool
    #: Journal symbol -> broker symbol, e.g. `NAS100` -> `US100Cash#`.
    symbols: dict[str, str] = field(default_factory=dict)

    # --- Garde-fous propres au pont -----------------------------------------
    # Ils s'appliquent toujours, y compris aux comptes sans prop firm derrière.
    # Une démo de broker n'a aucune règle, et doit rester impossible à faire
    # sauter par une faute de frappe.

    #: Part de l'équité qu'un plan peut risquer.
    risk_pct_max: float = DEFAULT_RISK_PCT_MAX
    #: Part du solde d'ouverture perdable avant que le pont n'accepte plus
    #: rien jusqu'au lendemain.
    daily_loss_pct_max: float = DEFAULT_DAILY_LOSS_PCT_MAX
    #: Plans, pas positions : un plan compte plusieurs jambes.
    max_open_plans: int = DEFAULT_MAX_OPEN_PLANS
    #: R moyen pondéré au-dessus duquel on exécute sans discuter.
    r_green: float = DEFAULT_R_GREEN
    #: R moyen pondéré en dessous duquel on refuse, sans dérogation.
    r_red: float = DEFAULT_R_RED

    def to_broker(self, name: str) -> str:
        """Translate a journal symbol into the broker's own spelling.

        Unknown names pass through untouched. The map exists to cover the
        instruments the journal tracks, not to forbid everything else — asking
        for a symbol the map has never heard of should reach the broker and
        fail there, with the broker's own error, rather than be rejected here
        with a misleading one.
        """
        return self.symbols.get(name.strip().upper(), name.strip())

    def to_journal(self, broker_name: str) -> str:
        """The reverse translation, for anything read back from the terminal.

        Several journal names could in principle map to one broker symbol; the
        first match wins, which is deterministic because TOML preserves order.
        """
        for journal, broker in self.symbols.items():
            if broker.casefold() == broker_name.casefold():
                return journal
        return broker_name


def load(path: Path | None = None) -> list[Account]:
    """Parse and validate the configuration.

    Validation is strict and happens once at start-up rather than at the first
    call: a typo in a terminal path should stop the service with a clear
    message, not surface three hours later as an empty position list.
    """
    path = path or DEFAULT_CONFIG
    if not path.is_file():
        raise ConfigError(
            f"configuration introuvable : {path}\n"
            f"copiez accounts.example.toml en accounts.toml et renseignez-la."
        )

    # Read as text with `utf-8-sig` rather than handing the file object to
    # `tomllib.load`. Every native Windows editor — Notepad, and PowerShell's
    # own `Set-Content -Encoding UTF8` — writes a byte-order mark, and
    # `tomllib` rejects it with "Invalid statement at line 1, column 1", which
    # points at the first line of a file whose first line is perfectly valid.
    # `utf-8-sig` strips the mark when present and changes nothing when it is
    # not.
    try:
        text = path.read_text(encoding="utf-8-sig")
    except UnicodeDecodeError as exc:
        raise ConfigError(
            f"{path} n'est pas un fichier UTF-8 : {exc}\n"
            f"réenregistrez-le en UTF-8 (sans BOM de préférence)."
        ) from exc

    try:
        raw = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise ConfigError(f"{path} : TOML invalide — {exc}") from exc

    entries = raw.get("account")
    if not entries:
        raise ConfigError(f"{path} ne contient aucune section [[account]]")

    accounts: list[Account] = []
    for index, entry in enumerate(entries):
        accounts.append(_parse_account(entry, index, path))

    _reject_duplicates(accounts, path)
    return accounts


def _parse_account(entry: dict, index: int, path: Path) -> Account:
    where = f"{path}, [[account]] n°{index + 1}"

    for key in ("alias", "login", "server", "terminal"):
        if not str(entry.get(key, "")).strip():
            raise ConfigError(f"{where} : champ '{key}' manquant ou vide")

    alias = str(entry["alias"]).strip()
    login = str(entry["login"]).strip()

    # Digits only, for the same reason the backend enforces it: the login is
    # the prefix of the ticket key the journal stores, so a separator smuggled
    # into it would let one terminal address another's rows.
    if not login.isdigit():
        raise ConfigError(f"{where} : 'login' doit être un numéro de compte MT5")

    terminal = Path(str(entry["terminal"]).strip())
    if not terminal.is_file():
        raise ConfigError(
            f"{where} : terminal introuvable à {terminal}\n"
            f"le chemin doit pointer sur terminal64.exe, pas sur son dossier."
        )

    symbols = entry.get("symbols", {})
    if not isinstance(symbols, dict):
        raise ConfigError(f"{where} : 'symbols' doit être une table")

    limits = entry.get("limits", {})
    if not isinstance(limits, dict):
        raise ConfigError(f"{where} : 'limits' doit être une table")

    risk_pct_max = _fraction(limits, "risk_pct_max", DEFAULT_RISK_PCT_MAX, where)
    daily_loss_pct_max = _fraction(
        limits, "daily_loss_pct_max", DEFAULT_DAILY_LOSS_PCT_MAX, where
    )
    r_green = _positive(limits, "r_green", DEFAULT_R_GREEN, where)
    r_red = _positive(limits, "r_red", DEFAULT_R_RED, where)

    if r_red > r_green:
        raise ConfigError(
            f"{where} : r_red ({r_red}) au-dessus de r_green ({r_green}) — "
            f"le plancher de refus doit être sous le seuil d'acceptation"
        )
    if risk_pct_max > daily_loss_pct_max:
        # Sinon un seul trade perdant dépasserait l'enveloppe du jour, et le
        # pont refuserait systématiquement avant d'avoir rien laissé passer.
        raise ConfigError(
            f"{where} : risk_pct_max ({risk_pct_max}) au-dessus de "
            f"daily_loss_pct_max ({daily_loss_pct_max}) : aucun plan ne pourrait "
            f"jamais passer"
        )

    max_open_plans = limits.get("max_open_plans", DEFAULT_MAX_OPEN_PLANS)
    if not isinstance(max_open_plans, int) or max_open_plans < 1:
        raise ConfigError(f"{where} : 'max_open_plans' doit être un entier ≥ 1")

    return Account(
        alias=alias,
        login=login,
        server=str(entry["server"]).strip(),
        terminal=terminal,
        currency=str(entry.get("currency", "")).strip().upper() or "USD",
        enabled=bool(entry.get("enabled", True)),
        symbols={str(k).strip().upper(): str(v).strip() for k, v in symbols.items()},
        risk_pct_max=risk_pct_max,
        daily_loss_pct_max=daily_loss_pct_max,
        max_open_plans=max_open_plans,
        r_green=r_green,
        r_red=r_red,
    )


def _fraction(table: dict, key: str, default: float, where: str) -> float:
    """A percentage written as a fraction: 0.01 is one percent.

    Rejecting anything above 1 catches the likeliest mistake by far — writing
    `1` for one percent, which would authorise risking the whole account on
    one trade.
    """
    value = _positive(table, key, default, where)
    # `>= 1` et pas `> 1` : une fraction de 1 vaut 100 % du compte, ce qui
    # n'est jamais une intention. Écrire « 1 » pour un pour cent est de loin
    # la faute la plus probable, et elle doit se voir dire exactement ça.
    if value >= 1:
        raise ConfigError(
            f"{where} : '{key}' vaut {value} ; c'est une fraction de l'équité, "
            f"1 % s'écrit 0.01"
        )
    return value


def _positive(table: dict, key: str, default: float, where: str) -> float:
    raw = table.get(key, default)
    if not isinstance(raw, (int, float)) or isinstance(raw, bool) or raw <= 0:
        raise ConfigError(f"{where} : '{key}' doit être un nombre strictement positif")
    return float(raw)


def _reject_duplicates(accounts: list[Account], path: Path) -> None:
    """Two entries sharing an alias, a login or a terminal are always a bug.

    A duplicated alias makes tool calls ambiguous. A duplicated login would
    have both terminals push trades into the same journal account, which the
    backend's unique index would reject anyway — better to say so here. Two
    entries on the same terminal directory would corrupt each other's
    portable configuration.
    """
    for field_name, values in (
        ("alias", [a.alias.casefold() for a in accounts]),
        ("login", [a.login for a in accounts]),
        ("terminal", [str(a.terminal).casefold() for a in accounts]),
    ):
        seen: set[str] = set()
        for value in values:
            if value in seen:
                raise ConfigError(f"{path} : '{field_name}' en double — {value}")
            seen.add(value)
