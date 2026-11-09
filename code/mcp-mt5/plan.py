"""Turning a trade idea into sized positions.

Pure arithmetic: no MT5, no network, no clock. Everything this module needs
arrives as arguments, which is why it can be tested exhaustively — and it has
to be, because every mistake here is expressed in money.

A plan opens **several positions at once** rather than one position closed in
pieces. The reason is survivability: each leg carries its own take profit,
deposited with the broker, so the exits fire even if the machine running this
code reboots mid-session. A single position whittled down by a watcher needs
that watcher alive at the exact minute price touches the target.

Two numbers decide everything downstream:

* **Value per point** — `tick_value / tick_size`, the money one lot gains per
  unit of price. It is read per symbol and never assumed: on XM a GER40 point
  is worth 1 USD a lot, a dollar of gold is worth 100.
* **Volume step** — the granularity the broker accepts. Also per symbol, also
  never assumed: 0.1 on XM's indices, 0.01 on its gold. Rounding gold to the
  tenth would open a position ten times too large.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal, ROUND_DOWN

LONG = "LONG"
SHORT = "SHORT"


class PlanError(Exception):
    """The plan cannot be built. The message is meant to be read by a person."""


@dataclass(frozen=True)
class SymbolSpec:
    """What the broker says about an instrument. Mirrors `get_symbol_spec`."""

    name: str
    digits: int
    point: float
    tick_size: float
    tick_value: float
    contract_size: float
    volume_min: float
    volume_step: float
    volume_max: float
    #: Minimum distance, in points, the broker imposes between price and a
    #: stop. Often zero, and often enforced anyway around news.
    stops_level: int = 0

    @property
    def value_per_price_unit(self) -> float:
        """Money one lot gains per 1.0 of price movement."""
        if self.tick_size <= 0:
            raise PlanError(f"{self.name} : tick_size nul, dimensionnement impossible")
        return self.tick_value / self.tick_size


@dataclass(frozen=True)
class Thresholds:
    """Where a degraded reward-to-risk stops being acceptable.

    Two levels rather than one, because a trade can be worth less than planned
    without being worthless. Between the two, a human decides.
    """

    #: At or above: proceed normally.
    green: float = 0.80
    #: Below: refuse outright, no override offered.
    red: float = 0.50


@dataclass(frozen=True)
class PlanWarning:
    """Something worth saying about a plan, and how loudly.

    The distinction matters more than it looks. A note that fires on every
    plan stops being read, and a confirmation that is always required stops
    being a decision — which is how the benefit of asking is lost. So only
    what the reward-to-risk cannot already express is allowed to demand a
    human yes.
    """

    code: str
    #: `info` is displayed and nothing more. `warn` requires an explicit yes.
    level: str
    message: str


@dataclass(frozen=True)
class Leg:
    index: int
    volume: float
    take_profit: float
    #: Reward divided by risk for this leg alone.
    r_multiple: float


@dataclass
class SizedPlan:
    symbol: str
    direction: str
    #: The price the sizing was actually computed from — the live price for a
    #: market entry, not the price the idea was written around.
    reference_price: float
    stop_loss: float
    stop_distance: float
    risk_amount: float
    risk_pct: float
    value_per_point: float
    total_volume: float
    legs: list[Leg]
    #: Volume-weighted reward-to-risk across the legs: the expectancy of the
    #: trade as it would actually be executed, not of its furthest target.
    blended_r: float
    #: The same figure at the entry originally asked for, when that differs.
    planned_blended_r: float | None
    zone: str  # "green" | "amber" | "red"
    warnings: list[PlanWarning] = field(default_factory=list)

    @property
    def degradation(self) -> float | None:
        """How much expectancy was lost between the plan and the live price."""
        if self.planned_blended_r in (None, 0):
            return None
        return 1 - self.blended_r / self.planned_blended_r


def build(
    *,
    spec: SymbolSpec,
    direction: str,
    stop_loss: float,
    targets: list[float],
    equity: float,
    risk_pct: float,
    market_price: float,
    entry: float | None = None,
    entry_range: tuple[float, float] | None = None,
    thresholds: Thresholds = Thresholds(),
) -> SizedPlan:
    """Size a trade and split it into legs.

    `market_price` is the price an order would actually fill at — ask for a
    long, bid for a short. Sizing from the price written in the signal instead
    would compute a risk the position does not have: the stop has not moved,
    so entering ten points higher makes the real loss ten points bigger than
    the one that was budgeted.
    """
    direction = _normalise_direction(direction)
    if not targets:
        raise PlanError("aucune cible : un plan a besoin d'au moins une sortie")
    if equity <= 0:
        raise PlanError("équité nulle ou négative")
    if not 0 < risk_pct <= 1:
        raise PlanError("le risque doit être une fraction de l'équité, entre 0 et 1")
    if market_price <= 0:
        raise PlanError("pas de cotation : le marché est fermé ou le symbole n'est pas coté")

    warnings: list[str] = []
    reference = _entry_reference(
        direction, market_price, entry, entry_range, stop_loss, warnings
    )

    _check_geometry(direction, reference, stop_loss, targets, spec)

    stop_distance = abs(reference - stop_loss)
    value_per_point = spec.value_per_price_unit
    risk_amount = equity * risk_pct

    total_volume = _size(
        risk_amount=risk_amount,
        stop_distance=stop_distance,
        value_per_point=value_per_point,
        spec=spec,
        leg_count=len(targets),
        warnings=warnings,
    )

    legs = _split(total_volume, targets, reference, stop_distance, spec)
    blended = _blended_r(legs, total_volume)

    # What the idea was worth at the price it was written around, so the cost
    # of arriving late is visible rather than inferred.
    planned = None
    planned_entry = _planned_entry(entry, entry_range, direction)
    if planned_entry is not None and abs(planned_entry - reference) > spec.point / 2:
        planned_distance = abs(planned_entry - stop_loss)
        if planned_distance > 0:
            planned = sum(
                abs(t - planned_entry) / planned_distance for t in targets
            ) / len(targets)

    # The achievable risk after rounding, which is what the trader is really
    # taking — never the number that was asked for.
    actual_risk = total_volume * stop_distance * value_per_point

    return SizedPlan(
        symbol=spec.name,
        direction=direction,
        reference_price=reference,
        stop_loss=stop_loss,
        stop_distance=stop_distance,
        risk_amount=round(actual_risk, 2),
        risk_pct=actual_risk / equity,
        value_per_point=value_per_point,
        total_volume=total_volume,
        legs=legs,
        blended_r=round(blended, 3),
        planned_blended_r=None if planned is None else round(planned, 3),
        zone=_zone(blended, thresholds),
        warnings=warnings,
    )


# --- Entry -------------------------------------------------------------------


def _entry_reference(
    direction: str,
    market_price: float,
    entry: float | None,
    entry_range: tuple[float, float] | None,
    stop_loss: float,
    warnings: list[PlanWarning],
) -> float:
    """Decide which price the plan is sized from, and flag how we got there.

    Always the live price: a market order fills there and nowhere else. The
    requested entry only serves to say whether we are early, on time or late.
    """
    if entry_range is not None:
        low, high = sorted(entry_range)
        if market_price > high:
            if direction == LONG:
                _note_drift(warnings, market_price - high)
            else:
                _warn_toward_stop(direction, warnings, market_price, high, stop_loss)
        elif market_price < low:
            if direction == SHORT:
                _note_drift(warnings, low - market_price)
            else:
                _warn_toward_stop(direction, warnings, market_price, low, stop_loss)
    elif entry is not None:
        drift = market_price - entry
        late = (direction == LONG and drift > 0) or (direction == SHORT and drift < 0)
        if late:
            _note_drift(warnings, abs(drift))
        elif abs(drift) > 0:
            _warn_toward_stop(direction, warnings, market_price, entry, stop_loss)

    return market_price


def _note_drift(warnings: list[PlanWarning], distance: float) -> None:
    """Say that the price moved away from the planned entry — and no more.

    Informational on purpose. The price always moves between reading an idea
    and pricing it, so demanding a confirmation for that alone would demand
    one on nearly every real plan, and a confirmation that is always required
    is one that gets clicked without reading.

    The drift has no consequence of its own: its consequence *is* the loss of
    reward-to-risk, which is already measured and already sorted into three
    zones. If the drift matters, the zone turns amber by itself and that is
    what asks for a human. Raising it here as well would count the same fact
    twice.
    """
    warnings.append(
        PlanWarning(
            code="drift",
            level="info",
            message=f"le prix s'est éloigné de {distance:.2f} depuis l'entrée prévue",
        )
    )


def _warn_toward_stop(
    direction: str,
    warnings: list[PlanWarning],
    market_price: float,
    edge: float,
    stop_loss: float,
) -> None:
    """Flag the case where a better price is a worse situation.

    Price has moved past the entry *toward* the stop. Mechanically the ratio
    improves — the stop is nearer, so the same targets are worth more R. That
    reading flatters a trade whose premise may already be failing, which is
    exactly when a number saying "excellent" is most dangerous.

    This one does demand a yes, because it is precisely what the ratio cannot
    say: the zone will read green while the setup is on its way to being
    invalidated.
    """
    between = (direction == LONG and stop_loss < market_price < edge) or (
        direction == SHORT and edge < market_price < stop_loss
    )
    if between:
        travelled = abs(edge - market_price) / max(abs(edge - stop_loss), 1e-9)
        warnings.append(
            PlanWarning(
                code="past_entry",
                level="warn",
                message=(
                    f"le prix est déjà passé de l'autre côté de l'entrée, à "
                    f"{travelled:.0%} du chemin vers le stop : le ratio s'améliore "
                    f"mécaniquement alors que le scénario se dégrade"
                ),
            )
        )


def _planned_entry(
    entry: float | None,
    entry_range: tuple[float, float] | None,
    direction: str,
) -> float | None:
    """The price the idea assumed, for the before-and-after comparison.

    For a range, the edge the trade *wanted*: the low for a long, the high for
    a short. Using the midpoint would understate what arriving late cost.
    """
    if entry is not None:
        return entry
    if entry_range is None:
        return None
    low, high = sorted(entry_range)
    return low if direction == LONG else high


# --- Geometry ----------------------------------------------------------------


def _check_geometry(
    direction: str,
    reference: float,
    stop_loss: float,
    targets: list[float],
    spec: SymbolSpec,
) -> None:
    """Refuse a plan whose prices contradict its direction.

    These are not edge cases to round off — a stop on the wrong side turns a
    protective order into a profit target, and the position would be closed at
    the first tick in its favour.
    """
    if direction == LONG and stop_loss >= reference:
        raise PlanError(
            f"stop {stop_loss} au-dessus du prix {reference} pour un achat : "
            f"le sens ou le stop est faux"
        )
    if direction == SHORT and stop_loss <= reference:
        raise PlanError(
            f"stop {stop_loss} en dessous du prix {reference} pour une vente : "
            f"le sens ou le stop est faux"
        )

    for index, target in enumerate(targets, start=1):
        wrong = (direction == LONG and target <= reference) or (
            direction == SHORT and target >= reference
        )
        if wrong:
            raise PlanError(
                f"cible {index} ({target}) déjà atteinte ou du mauvais côté "
                f"du prix {reference}"
            )

    ordered = sorted(targets, reverse=(direction == SHORT))
    if ordered != targets:
        raise PlanError(
            "les cibles ne sont pas dans l'ordre : la plus proche doit venir en premier"
        )

    if spec.stops_level > 0:
        minimum = spec.stops_level * spec.point
        if abs(reference - stop_loss) < minimum:
            raise PlanError(
                f"stop à {abs(reference - stop_loss):.2f} du prix, le broker en "
                f"exige {minimum:.2f} au minimum"
            )


# --- Sizing ------------------------------------------------------------------


def _size(
    *,
    risk_amount: float,
    stop_distance: float,
    value_per_point: float,
    spec: SymbolSpec,
    leg_count: int,
    warnings: list[PlanWarning],
) -> float:
    """Volume that risks `risk_amount` if the stop is hit, rounded to the step.

    Rounded **down**, always. Rounding up would exceed the risk that was
    explicitly asked for, and a guard that quietly overshoots its own limit is
    worse than no guard.
    """
    if stop_distance <= 0:
        raise PlanError("distance au stop nulle")

    raw = risk_amount / (stop_distance * value_per_point)
    volume = _floor_to_step(raw, spec.volume_step)

    if volume > spec.volume_max:
        # Demands a yes: the trade no longer carries the risk that was asked
        # for, and nothing else in the output says so.
        warnings.append(
            PlanWarning(
                code="volume_capped",
                level="warn",
                message=(
                    f"volume plafonné à {spec.volume_max} par le broker : le risque "
                    f"réel sera inférieur à celui demandé"
                ),
            )
        )
        volume = _floor_to_step(spec.volume_max, spec.volume_step)

    if volume < spec.volume_min:
        raise PlanError(
            f"le risque demandé donne {raw:.4f} lot, sous le minimum de "
            f"{spec.volume_min} du broker. Élargissez le risque ou resserrez le stop."
        )

    needed = spec.volume_min * leg_count
    if volume < needed - 1e-9:
        raise PlanError(
            f"{volume} lot ne se découpe pas en {leg_count} jambes : il en faut "
            f"au moins {_trim(needed)}. Réduisez le nombre de cibles ou augmentez "
            f"le risque."
        )

    return volume


def _split(
    total_volume: float,
    targets: list[float],
    reference: float,
    stop_distance: float,
    spec: SymbolSpec,
) -> list[Leg]:
    """Divide the volume across the targets, remainder on the last leg.

    Counted in whole steps rather than in lots: dividing 20.0 by 3 in floating
    point gives 6.666666666666667, and three of those do not add back to 20.
    A broker rejecting the last leg for a volume off by 10^-15 is a bad way to
    discover that.

    The remainder lands on the **furthest** target, which is the leg most
    likely to be left running — so the extra size rides the move rather than
    being banked first.
    """
    step = Decimal(str(spec.volume_step))
    total_steps = int((Decimal(str(total_volume)) / step).to_integral_value(ROUND_DOWN))
    count = len(targets)

    per_leg = total_steps // count
    legs: list[Leg] = []
    for index, target in enumerate(targets, start=1):
        steps = per_leg if index < count else total_steps - per_leg * (count - 1)
        volume = float(step * steps)
        legs.append(
            Leg(
                index=index,
                volume=_trim(volume),
                take_profit=target,
                r_multiple=round(abs(target - reference) / stop_distance, 3),
            )
        )
    return legs


def _blended_r(legs: list[Leg], total_volume: float) -> float:
    """Volume-weighted reward-to-risk: the expectancy of the whole plan.

    Weighted, not averaged. With equal legs the two agree, but the remainder
    sits on the furthest target, so a simple mean would understate a plan
    whose largest leg aims highest.
    """
    if total_volume <= 0:
        return 0.0
    return sum(leg.volume * leg.r_multiple for leg in legs) / total_volume


def _zone(blended_r: float, thresholds: Thresholds) -> str:
    if blended_r >= thresholds.green:
        return "green"
    if blended_r >= thresholds.red:
        return "amber"
    return "red"


# --- Numeric helpers ----------------------------------------------------------


def _floor_to_step(value: float, step: float) -> float:
    if step <= 0:
        raise PlanError("pas de volume nul")
    quantum = Decimal(str(step))
    steps = (Decimal(str(value)) / quantum).to_integral_value(ROUND_DOWN)
    return _trim(float(quantum * steps))


def _trim(value: float) -> float:
    """Drop the floating-point dust that makes 0.1 + 0.2 fail a broker check."""
    return float(Decimal(str(value)).normalize().quantize(Decimal("0.00000001")).normalize())


def _normalise_direction(direction: str) -> str:
    value = (direction or "").strip().upper()
    if value in (LONG, "BUY", "ACHAT"):
        return LONG
    if value in (SHORT, "SELL", "VENTE"):
        return SHORT
    raise PlanError(f"sens inconnu : '{direction}'")
