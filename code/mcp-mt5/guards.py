"""What stands between a sized plan and an order.

Two layers, because they protect against different things and neither
replaces the other.

The **bridge's own ceilings** come from `accounts.toml` and always apply:
maximum risk per trade, maximum loss per day, maximum plans open at once.
They exist for accounts with no prop firm behind them — the XM demo has no
rules, and must still be impossible to blow up through a typo.

The **prop firm's rules** come from the journal's `/accounts/:id/prop-status`
and only apply where an account declares them. They are the ones that end a
challenge, and the bridge cuts in before they are reached rather than at the
moment they are breached.

Like `plan.py`, nothing here touches MT5 or the network. Everything arrives as
an argument so every refusal can be provoked in a test.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from plan import SizedPlan

#: Fraction of a limit above which the trader is warned but not stopped.
WARN_AT = 0.8

#: Rounding slack, so a plan landing exactly on a limit is not refused by a
#: floating-point remainder in the fifteenth decimal.
EPSILON = 1e-6


@dataclass(frozen=True)
class Limits:
    """The bridge's own ceilings. Per account, from `accounts.toml`."""

    #: Share of equity a single plan may risk.
    risk_pct_max: float = 0.01
    #: Share of the day's opening balance that may be lost before the bridge
    #: stops accepting plans until tomorrow.
    daily_loss_pct_max: float = 0.02
    #: Plans — not positions. One plan is several legs.
    max_open_plans: int = 1


@dataclass(frozen=True)
class AccountState:
    """The broker's view, which is the one that counts for live limits."""

    equity: float
    balance: float
    currency: str
    margin_free: float
    #: Realised today, fees included. Negative when losing.
    today_realised_pnl: float
    open_plans: int
    open_positions: int
    #: `False` when the terminal's algo trading switch is off.
    algo_trading_enabled: bool = True
    #: `full`, or anything else when the symbol is restricted.
    trade_mode: str = "full"
    #: `False` when no tick has arrived — the weekend, typically.
    quoting: bool = True

    @property
    def opening_balance(self) -> float:
        """Balance at the start of the day, which the daily limit is a share of.

        Derived rather than stored: the bridge may be started at any hour, so
        it cannot have watched midnight go by. Today's realised result is
        known, and subtracting it from the current balance gives where the day
        began.
        """
        return self.balance - self.today_realised_pnl


@dataclass
class Finding:
    #: `block` refuses outright. `warn` is shown and requires the human to
    #: say yes explicitly.
    level: str
    code: str
    message: str


@dataclass
class Verdict:
    findings: list[Finding] = field(default_factory=list)

    @property
    def blocked(self) -> bool:
        return any(f.level == "block" for f in self.findings)

    @property
    def needs_confirmation(self) -> bool:
        return any(f.level == "warn" for f in self.findings)

    @property
    def decision(self) -> str:
        if self.blocked:
            return "refusé"
        if self.needs_confirmation:
            return "à confirmer"
        return "accepté"

    def block(self, code: str, message: str) -> None:
        self.findings.append(Finding("block", code, message))

    def warn(self, code: str, message: str) -> None:
        self.findings.append(Finding("warn", code, message))


def check(
    *,
    sized: SizedPlan,
    limits: Limits,
    state: AccountState,
    prop: dict | None = None,
    margin_required: float | None = None,
) -> Verdict:
    """Run every guard against a sized plan.

    Returns all findings rather than the first: a trader who fixes one
    refusal only to meet the next has learned nothing about where the real
    wall is.
    """
    verdict = Verdict()

    _market(verdict, state)
    _ratio(verdict, sized)
    _risk(verdict, sized, limits, state)
    _daily(verdict, sized, limits, state)
    _concurrency(verdict, limits, state)
    _margin(verdict, state, margin_required)
    _prop(verdict, sized, prop)

    for warning in sized.warnings:
        verdict.warn("plan", warning)

    return verdict


def _market(verdict: Verdict, state: AccountState) -> None:
    if not state.quoting:
        verdict.block(
            "no_quote",
            "aucune cotation : le marché est fermé ou le symbole n'est pas suivi",
        )
    if state.trade_mode != "full":
        verdict.block(
            "trade_mode",
            f"le symbole est en mode '{state.trade_mode}', pas en trading complet",
        )
    if not state.algo_trading_enabled:
        verdict.block(
            "algo_off",
            "le bouton Algo Trading du terminal est éteint : tout ordre serait rejeté",
        )


def _ratio(verdict: Verdict, sized: SizedPlan) -> None:
    if sized.zone == "red":
        verdict.block(
            "ratio",
            f"ratio moyen pondéré de {sized.blended_r} : sous le plancher, "
            f"l'espérance n'est plus défendable",
        )
    elif sized.zone == "amber":
        lost = sized.degradation
        detail = f", soit {lost:.0%} de l'espérance prévue perdue" if lost else ""
        verdict.warn(
            "ratio",
            f"ratio moyen pondéré de {sized.blended_r}{detail}",
        )


def _risk(
    verdict: Verdict, sized: SizedPlan, limits: Limits, state: AccountState
) -> None:
    if sized.risk_pct > limits.risk_pct_max + EPSILON:
        verdict.block(
            "risk_per_trade",
            f"risque de {sized.risk_pct:.2%} de l'équité, plafond à "
            f"{limits.risk_pct_max:.2%}",
        )


def _daily(
    verdict: Verdict, sized: SizedPlan, limits: Limits, state: AccountState
) -> None:
    allowance = state.opening_balance * limits.daily_loss_pct_max
    if allowance <= 0:
        return

    lost = max(0.0, -state.today_realised_pnl)
    if lost >= allowance - EPSILON:
        verdict.block(
            "daily_spent",
            f"{lost:,.0f} {state.currency} perdus aujourd'hui sur une enveloppe "
            f"de {allowance:,.0f} : la journée est terminée",
        )
        return

    # The guard that matters: refuse a plan whose *full* loss would breach the
    # day, rather than discovering it once the stop is hit. A limit checked
    # only after the fact is a report, not a limit.
    projected = lost + sized.risk_amount
    if projected > allowance + EPSILON:
        verdict.block(
            "daily_would_breach",
            f"ce plan risque {sized.risk_amount:,.0f} {state.currency} ; ajouté aux "
            f"{lost:,.0f} déjà perdus il dépasserait l'enveloppe du jour "
            f"({allowance:,.0f})",
        )
    elif lost >= allowance * WARN_AT:
        verdict.warn(
            "daily_close",
            f"{lost / allowance:.0%} de l'enveloppe journalière déjà consommée",
        )


def _concurrency(verdict: Verdict, limits: Limits, state: AccountState) -> None:
    if state.open_plans >= limits.max_open_plans:
        verdict.block(
            "too_many_plans",
            f"{state.open_plans} plan(s) déjà ouvert(s) pour un maximum de "
            f"{limits.max_open_plans}",
        )


def _margin(
    verdict: Verdict, state: AccountState, margin_required: float | None
) -> None:
    if margin_required is None:
        return
    if margin_required > state.margin_free:
        verdict.block(
            "margin",
            f"marge requise {margin_required:,.0f} {state.currency} pour "
            f"{state.margin_free:,.0f} disponibles",
        )
    elif margin_required > state.margin_free * 0.5:
        # Not a refusal, but worth saying: half the free margin in one plan
        # leaves little room for the position to breathe before a margin call
        # closes it at the broker's choosing rather than at the stop.
        verdict.warn(
            "margin_tight",
            f"ce plan immobilise {margin_required / state.margin_free:.0%} "
            f"de la marge libre",
        )


def _prop(verdict: Verdict, sized: SizedPlan, prop: dict | None) -> None:
    """Rules declared by a prop firm, read from the journal.

    Absent for an account that declares none, which is the normal case on a
    broker demo — and precisely why the bridge has ceilings of its own.
    """
    if not prop:
        return

    daily_used = _as_float(prop.get("daily_used"))
    if daily_used is not None:
        if daily_used >= 1.0:
            verdict.block("prop_daily", "limite de perte journalière de la firme dépassée")
        elif daily_used >= WARN_AT:
            verdict.warn(
                "prop_daily_close",
                f"{daily_used:.0%} de la limite journalière de la firme consommée",
            )

    drawdown_used = _as_float(prop.get("drawdown_used"))
    if drawdown_used is not None:
        if drawdown_used >= 1.0:
            verdict.block("prop_drawdown", "drawdown global de la firme dépassé")
        elif drawdown_used >= WARN_AT:
            verdict.warn(
                "prop_drawdown_close",
                f"{drawdown_used:.0%} du drawdown global de la firme consommé",
            )

    # A plan that would, on its own, take the account past the firm's daily
    # limit. Expressed in money, because the firm's limit is a share of the
    # starting balance while the plan's risk is a share of current equity —
    # comparing the two percentages directly would be wrong.
    daily_limit = _as_float(prop.get("daily_limit"))
    today_pnl = _as_float(prop.get("today_pnl"))
    if daily_limit and daily_limit > 0 and today_pnl is not None:
        lost = max(0.0, -today_pnl)
        if lost + sized.risk_amount > daily_limit + EPSILON and not verdict.blocked:
            verdict.block(
                "prop_daily_would_breach",
                f"ce plan ferait passer la perte du jour au-delà de la limite de "
                f"la firme ({daily_limit:,.0f})",
            )

    if prop.get("consistency_ok") is False:
        verdict.warn(
            "prop_consistency",
            "la règle de consistance est déjà enfreinte : le payout est bloqué "
            "tant que le total n'a pas rattrapé la meilleure journée",
        )


def _as_float(value: object) -> float | None:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    return None
