"""Tests des garde-fous.

`py -3.12 test_guards.py`, ou via pytest. Chaque refus doit être provocable :
un garde-fou qu'on ne sait pas déclencher en test est un garde-fou qu'on
découvrira cassé le jour où il devait servir.
"""

from __future__ import annotations

import guards
import plan
from guards import AccountState, Limits
from test_plan import GER40


def sized(**kwargs):
    base = dict(
        spec=GER40,
        direction="LONG",
        stop_loss=26_250.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        equity=100_000.0,
        risk_pct=0.01,
        market_price=26_300.0,
    )
    base.update(kwargs)
    return plan.build(**base)


def state(**kwargs):
    base = dict(
        equity=100_000.0,
        balance=100_000.0,
        currency="USD",
        margin_free=90_000.0,
        today_realised_pnl=0.0,
        open_plans=0,
        open_positions=0,
    )
    base.update(kwargs)
    return AccountState(**base)


LIMITS = Limits(risk_pct_max=0.01, daily_loss_pct_max=0.02, max_open_plans=1)


def check(**kwargs):
    base = dict(sized=sized(), limits=LIMITS, state=state())
    base.update(kwargs)
    return guards.check(**base)


def codes(verdict):
    return {f.code for f in verdict.findings}


# --- Le cas nominal ----------------------------------------------------------


def test_un_plan_propre_passe_sans_rien_dire():
    v = check()
    assert v.decision == "accepté"
    assert v.findings == []


# --- État du marché ----------------------------------------------------------


def test_sans_cotation_rien_ne_part():
    v = check(state=state(quoting=False))
    assert v.blocked
    assert "no_quote" in codes(v)


def test_algo_trading_eteint_bloque():
    # MT5 rejetterait l'ordre ; autant le dire avant de l'envoyer.
    v = check(state=state(algo_trading_enabled=False))
    assert "algo_off" in codes(v)


def test_un_symbole_en_fermeture_seule_bloque():
    v = check(state=state(trade_mode="close_only"))
    assert "trade_mode" in codes(v)


# --- Ratio -------------------------------------------------------------------


def test_la_zone_rouge_refuse():
    tardif = sized(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry=26_300.0,
        market_price=26_340.0,
    )
    v = check(sized=tardif)
    assert v.blocked
    assert "ratio" in codes(v)


def test_la_zone_orange_demande_confirmation_sans_refuser():
    tardif = sized(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry=26_300.0,
        market_price=26_320.0,
    )
    v = check(sized=tardif)
    assert not v.blocked
    assert v.needs_confirmation
    assert v.decision == "à confirmer"


# --- Risque par trade --------------------------------------------------------


def test_un_risque_au_dessus_du_plafond_refuse():
    v = check(sized=sized(risk_pct=0.02))
    assert "risk_per_trade" in codes(v)


def test_un_risque_pile_sur_le_plafond_passe():
    # Un plan refusé pour un reste de virgule flottante serait incompréhensible.
    v = check(sized=sized(risk_pct=0.01))
    assert not v.blocked


# --- Perte journalière -------------------------------------------------------


def test_lenveloppe_du_jour_se_calcule_sur_le_solde_douverture():
    # 2 000 déjà perdus : la journée a ouvert à 102 000, l'enveloppe vaut
    # 2 040 et non 2 000.
    s = state(balance=98_000.0, today_realised_pnl=-2_000.0)
    assert s.opening_balance == 100_000.0


def test_une_enveloppe_journaliere_epuisee_ferme_la_journee():
    v = check(state=state(balance=98_000.0, today_realised_pnl=-2_000.0))
    assert v.blocked
    assert "daily_spent" in codes(v)


def test_un_plan_qui_depasserait_lenveloppe_est_refuse_avant_lenvoi():
    # 1 500 perdus, enveloppe 2 000, plan à 1 000 : le stop ferait 2 500.
    # Un plafond vérifié seulement après coup est un rapport, pas un plafond.
    v = check(state=state(balance=98_500.0, today_realised_pnl=-1_500.0))
    assert v.blocked
    assert "daily_would_breach" in codes(v)


def test_deux_pertes_pleines_consecutives_tiennent_dans_lenveloppe():
    # Le réglage choisi : 1 % par trade, 2 % par jour. La deuxième passe.
    v = check(state=state(balance=99_000.0, today_realised_pnl=-1_000.0))
    assert not v.blocked, [f.message for f in v.findings]


def test_la_troisieme_est_refusee():
    v = check(state=state(balance=98_000.0, today_realised_pnl=-2_000.0))
    assert v.blocked


def test_une_journee_gagnante_ne_consomme_rien():
    v = check(state=state(balance=103_000.0, today_realised_pnl=3_000.0))
    assert not v.blocked
    assert "daily_close" not in codes(v)


# --- Plans simultanés --------------------------------------------------------


def test_un_plan_deja_ouvert_bloque_le_suivant():
    v = check(state=state(open_plans=1))
    assert v.blocked
    assert "too_many_plans" in codes(v)


def test_le_plafond_de_plans_est_configurable():
    v = check(limits=Limits(max_open_plans=3), state=state(open_plans=1))
    assert "too_many_plans" not in codes(v)


# --- Marge -------------------------------------------------------------------


def test_une_marge_insuffisante_refuse():
    v = check(margin_required=95_000.0)
    assert v.blocked
    assert "margin" in codes(v)


def test_une_marge_serree_avertit_sans_refuser():
    v = check(margin_required=60_000.0)
    assert not v.blocked
    assert "margin_tight" in codes(v)


def test_une_marge_inconnue_ne_declenche_rien():
    v = check(margin_required=None)
    assert "margin" not in codes(v)
    assert "margin_tight" not in codes(v)


# --- Règles prop firm --------------------------------------------------------


def test_sans_regles_declarees_la_couche_firme_est_muette():
    v = check(prop=None)
    assert v.findings == []


def test_la_limite_journaliere_de_la_firme_depassee_bloque():
    v = check(prop={"daily_used": 1.1, "drawdown_used": 0.2})
    assert "prop_daily" in codes(v)


def test_approcher_la_limite_de_la_firme_avertit():
    v = check(prop={"daily_used": 0.85, "drawdown_used": 0.2})
    assert not v.blocked
    assert "prop_daily_close" in codes(v)


def test_le_drawdown_global_de_la_firme_bloque():
    v = check(prop={"drawdown_used": 1.0})
    assert "prop_drawdown" in codes(v)


def test_un_plan_qui_ferait_sauter_la_limite_de_la_firme_est_refuse():
    # Limite de la firme à 5 000, 4 500 déjà perdus, plan à 1 000.
    v = check(prop={"daily_limit": 5_000.0, "today_pnl": -4_500.0, "daily_used": 0.9})
    assert v.blocked
    assert "prop_daily_would_breach" in codes(v)


def test_la_consistance_enfreinte_avertit_sans_bloquer():
    # Elle n'empêche pas de trader, elle bloque le retrait.
    v = check(prop={"consistency_ok": False})
    assert not v.blocked
    assert "prop_consistency" in codes(v)


def test_les_deux_couches_sont_independantes():
    # La firme est large, le pont est strict : c'est le pont qui tranche.
    v = check(
        state=state(balance=98_500.0, today_realised_pnl=-1_500.0),
        prop={"daily_used": 0.3, "drawdown_used": 0.1},
    )
    assert v.blocked
    assert "daily_would_breach" in codes(v)


# --- Restitution -------------------------------------------------------------


def test_tous_les_refus_sont_rendus_pas_seulement_le_premier():
    # Corriger un refus pour tomber sur le suivant n'apprend rien sur l'endroit
    # où est vraiment le mur.
    v = check(
        sized=sized(risk_pct=0.02),
        state=state(open_plans=1, quoting=False),
    )
    assert {"risk_per_trade", "too_many_plans", "no_quote"} <= codes(v)


def test_une_derive_simple_ninterrompt_pas_le_verdict():
    # Le défaut trouvé au premier essai en conditions réelles : une dérive de
    # 53 centimes faisait passer un plan vert en « à confirmer ».
    tardif = sized(entry=26_280.0, market_price=26_300.0)
    v = check(sized=tardif)
    assert any(f.code == "drift" and f.level == "info" for f in v.findings)
    assert not v.needs_confirmation
    assert v.decision == "accepté"


def test_un_prix_passe_de_lautre_cote_demande_toujours_un_oui():
    # Celui-là dit ce que le ratio ne peut pas dire : la zone est verte alors
    # que le scénario est en train d'être invalidé.
    retourne = sized(entry=26_320.0, market_price=26_300.0)
    v = check(sized=retourne)
    assert any(f.code == "past_entry" and f.level == "warn" for f in v.findings)
    assert v.decision == "à confirmer"


if __name__ == "__main__":
    import sys
    import traceback

    cases = [(n, f) for n, f in sorted(globals().items()) if n.startswith("test_")]
    failed = 0
    for name, case in cases:
        try:
            case()
            print(f"  ok   {name}")
        except Exception:
            failed += 1
            print(f"  ÉCHEC {name}")
            traceback.print_exc()
    print(f"\n{len(cases) - failed}/{len(cases)} tests passent")
    sys.exit(1 if failed else 0)
