"""Tests de la règle de remontée des stops.

    py -3.12 test_watcher.py

La règle tient en une phrase — dès qu'une jambe sort en gain, les suivantes
passent au point mort — mais ses cas limites décident de la différence entre
protéger une position et élargir son risque sans le voir.
"""

from __future__ import annotations

from watcher import OpenLeg, PlanState, breakeven_moves, group_by_plan


def leg(ticket: str, stop: float | None = 4084.30, volume: float = 0.42) -> OpenLeg:
    return OpenLeg(
        ticket=ticket, volume=volume, open_price=4092.08,
        stop_loss=stop, take_profit=4100.30,
    )


def plan(**kwargs) -> PlanState:
    base = dict(
        magic=760430423, symbol="GOLD#", direction="LONG",
        open_legs=[leg("2"), leg("3")],
        closed_in_profit=1, closed_at_loss=0,
    )
    base.update(kwargs)
    return PlanState(**base)


# --- Déclenchement -------------------------------------------------------------


def test_rien_ne_bouge_avant_la_premiere_sortie_en_gain():
    assert breakeven_moves(plan(closed_in_profit=0)) == []


def test_une_premiere_cible_touchee_remonte_les_jambes_restantes():
    mouvements = breakeven_moves(plan())
    assert [t for t, _ in mouvements] == ["2", "3"]
    assert all(stop == 4092.08 for _, stop in mouvements)


def test_une_sortie_au_stop_ne_declenche_rien():
    # Le stop a fait son travail ; il n'y a plus rien à protéger, et remonter
    # les jambes restantes au point mort après une perte reviendrait à les
    # fermer sur le moindre retour.
    assert breakeven_moves(plan(closed_in_profit=0, closed_at_loss=1)) == []


def test_un_plan_entierement_sorti_na_plus_de_stop_a_bouger():
    assert breakeven_moves(plan(open_legs=[])) == []


# --- Sens -----------------------------------------------------------------------


def test_sur_un_achat_seul_un_stop_plus_haut_est_une_amelioration():
    # Stop déjà au-dessus de l'entrée : ne pas le redescendre.
    deja_mieux = plan(open_legs=[leg("2", stop=4095.00)])
    assert breakeven_moves(deja_mieux) == []


def test_sur_une_vente_cest_exactement_linverse():
    vente = PlanState(
        magic=1, symbol="GOLD#", direction="SHORT",
        open_legs=[
            OpenLeg(ticket="2", volume=0.42, open_price=4092.08,
                    stop_loss=4100.00, take_profit=4084.00),
        ],
        closed_in_profit=1, closed_at_loss=0,
    )
    assert breakeven_moves(vente) == [("2", 4092.08)]

    deja_mieux = PlanState(
        magic=1, symbol="GOLD#", direction="SHORT",
        open_legs=[
            OpenLeg(ticket="2", volume=0.42, open_price=4092.08,
                    stop_loss=4089.00, take_profit=4084.00),
        ],
        closed_in_profit=1, closed_at_loss=0,
    )
    assert breakeven_moves(deja_mieux) == []


def test_une_jambe_sans_stop_est_toujours_amelioree():
    assert breakeven_moves(plan(open_legs=[leg("2", stop=None)])) == [("2", 4092.08)]


# --- Décalage ------------------------------------------------------------------


def test_le_decalage_pousse_le_stop_au_dela_de_lentree():
    # Zéro n'est pas tout à fait le point mort : l'entrée s'est faite à l'ask
    # et le stop se déclenche sur le bid, donc sortir à l'entrée exacte coûte
    # encore le spread.
    mouvements = breakeven_moves(plan(), offset=0.24)
    assert all(abs(stop - 4092.32) < 1e-9 for _, stop in mouvements)


def test_sur_une_vente_le_decalage_va_dans_lautre_sens():
    vente = PlanState(
        magic=1, symbol="GOLD#", direction="SHORT",
        open_legs=[OpenLeg("2", 0.42, 4092.08, 4100.00, 4084.00)],
        closed_in_profit=1, closed_at_loss=0,
    )
    assert breakeven_moves(vente, offset=0.24) == [("2", 4091.84)]


# --- Prix d'entrée --------------------------------------------------------------


def test_lentree_est_ponderee_par_le_volume():
    melange = plan(open_legs=[
        OpenLeg("2", volume=1.0, open_price=4092.00, stop_loss=4084.0, take_profit=4100.0),
        OpenLeg("3", volume=3.0, open_price=4093.00, stop_loss=4084.0, take_profit=4104.0),
    ])
    # (4092×1 + 4093×3) / 4 = 4092,75
    assert melange.entry_price == 4092.75


def test_un_plan_sans_jambe_ouverte_na_pas_de_prix_dentree():
    assert plan(open_legs=[]).entry_price is None


# --- Regroupement ---------------------------------------------------------------


def positions_brutes(magic: int = 760430423) -> list[dict]:
    return [
        {"ticket": "1", "symbol": "GOLD#", "direction": "LONG", "volume": 0.42,
         "open_price": 4092.08, "stop_loss": 4084.3, "take_profit": 4096.3, "magic": magic},
        {"ticket": "2", "symbol": "GOLD#", "direction": "LONG", "volume": 0.42,
         "open_price": 4092.08, "stop_loss": 4084.3, "take_profit": 4100.3, "magic": magic},
    ]


def test_les_positions_se_regroupent_par_magic():
    plans = group_by_plan(positions_brutes(), [])
    assert list(plans) == [760430423]
    assert len(plans[760430423].open_legs) == 2


def test_les_positions_ouvertes_a_la_main_sont_ignorees():
    # Le watcher ne touche qu'à ce qu'il a posé lui-même. Déplacer le stop
    # d'une position ouverte à la main serait agir sur une décision qui n'est
    # pas la sienne.
    manuelles = positions_brutes(magic=0)
    assert group_by_plan(manuelles, []) == {}


def test_lhistorique_compte_les_sorties_gagnantes_et_perdantes():
    histoire = [
        {"magic": 760430423, "closed": True, "profit": 168.0},
        {"magic": 760430423, "closed": True, "profit": -420.0},
        {"magic": 999, "closed": True, "profit": 50.0},       # autre plan
        {"magic": 760430423, "closed": False, "profit": 0.0},  # encore ouvert
    ]
    etat = group_by_plan(positions_brutes(), histoire)[760430423]
    assert etat.closed_in_profit == 1
    assert etat.closed_at_loss == 1


def test_un_plan_dont_tout_est_sorti_nest_pas_reconstitue():
    # Sans position ouverte il n'y a rien à modifier, donc rien à suivre.
    assert group_by_plan([], [{"magic": 760430423, "closed": True, "profit": 168.0}]) == {}


# --- Le scénario complet --------------------------------------------------------


def test_le_scenario_du_plan_a_trois_jambes():
    # Trois jambes posées, la première sort sur sa cible : les deux autres
    # passent au point mort et le plan ne peut plus perdre.
    positions = [
        {"ticket": "2", "symbol": "GOLD#", "direction": "LONG", "volume": 0.42,
         "open_price": 4092.08, "stop_loss": 4084.3, "take_profit": 4100.3,
         "magic": 760430423},
        {"ticket": "3", "symbol": "GOLD#", "direction": "LONG", "volume": 0.44,
         "open_price": 4092.08, "stop_loss": 4084.3, "take_profit": 4104.3,
         "magic": 760430423},
    ]
    histoire = [{"magic": 760430423, "closed": True, "profit": 176.4}]

    etat = group_by_plan(positions, histoire)[760430423]
    mouvements = breakeven_moves(etat)
    assert mouvements == [("2", 4092.08), ("3", 4092.08)]

    # Deuxième tour : les stops sont déjà au point mort, plus rien à envoyer.
    apres = [{**p, "stop_loss": 4092.08} for p in positions]
    etat = group_by_plan(apres, histoire)[760430423]
    assert breakeven_moves(etat) == []

# --- Remontée vers le journal ---------------------------------------------------

import journal as journal_module


def vers_journal(nom: str) -> str:
    return {"GOLD#": "XAUUSD", "GER40Cash#": "GER40"}.get(nom, nom)


def test_le_symbole_pousse_est_celui_du_journal():
    # GOLD# chez XM, XAUUSD partout ailleurs. Pousser le nom du broker
    # couperait l'or en deux séries selon le terminal qui l'a produit.
    lot = journal_module.to_trades(positions_brutes(), [], vers_journal)
    assert {t["symbol"] for t in lot} == {"XAUUSD"}


def test_lidentifiant_de_plan_est_le_magic():
    # Une position MT5 ne porte que 31 caractères de commentaire : l'UUID n'y
    # tient pas. Le magic, lui, est sur chaque position et chaque deal.
    lot = journal_module.to_trades(positions_brutes(), [], vers_journal)
    assert {t["plan_id"] for t in lot} == {"760430423"}


def test_ce_qui_na_pas_de_magic_nest_pas_remonte():
    # Le pont ne remonte que les plans qu'il a posés.
    assert journal_module.to_trades(positions_brutes(magic=0), [], vers_journal) == []
    histoire = [{"magic": 0, "closed": True, "ticket": "9", "symbol": "GOLD#",
                 "direction": "LONG", "profit": 10.0}]
    assert journal_module.to_trades([], histoire, vers_journal) == []


def test_une_position_ouverte_porte_son_stop_et_sa_cible():
    lot = journal_module.to_trades(positions_brutes(), [], vers_journal)
    assert lot[0]["stop_loss"] == 4084.3
    assert lot[0]["take_profit"] == 4096.3
    assert "close_time" not in lot[0]


def test_un_trade_cloture_na_plus_de_stop_et_ce_nest_pas_grave():
    # MT5 ne conserve pas le stop d'une position fermée. L'upsert du backend
    # ne remplace jamais une valeur connue par une absence, donc le stop
    # enregistré pendant que la position vivait survit à sa clôture.
    histoire = [{
        "ticket": "1", "symbol": "GOLD#", "direction": "LONG", "volume": 0.42,
        "open_time": "2026-11-09T08:05:50", "close_time": "2026-11-09T09:12:00",
        "open_price": 4092.08, "close_price": 4096.30, "profit": 177.24,
        "commission": -2.0, "swap": 0.0, "magic": 760430423, "closed": True,
    }]
    lot = journal_module.to_trades([], histoire, vers_journal)
    assert len(lot) == 1
    assert "stop_loss" not in lot[0]
    assert lot[0]["close_price"] == 4096.30
    assert lot[0]["commission"] == -2.0


def test_ouvertes_et_cloturees_partent_dans_le_meme_lot():
    histoire = [{"ticket": "1", "symbol": "GOLD#", "direction": "LONG",
                 "volume": 0.42, "profit": 177.24, "magic": 760430423,
                 "closed": True}]
    lot = journal_module.to_trades(positions_brutes(), histoire, vers_journal)
    assert [t["ticket"] for t in lot] == ["1", "2", "1"]


def test_un_lot_vide_nappelle_pas_le_reseau():
    class Muet(journal_module.Journal):
        def __init__(self):
            super().__init__("http://inexistant", "x")

    assert Muet().push("1", []) == {"received": 0, "created": 0, "updated": 0}

# --- Enregistrement dès l'envoi -------------------------------------------------

import plan as plan_module
from test_plan import GOLD


class _Compte:
    alias = "xm-demo"
    login = "169494134"

    def to_journal(self, nom):
        return {"GOLD#": "XAUUSD"}.get(nom, nom)


def _plan_et_envoi():
    sized = plan_module.build(
        spec=GOLD, direction="LONG", stop_loss=4084.30,
        targets=[4096.30, 4100.30, 4104.30],
        equity=100_000.0, risk_pct=0.01, market_price=4092.07,
    )
    envoi = {"legs": [
        {"leg": 1, "order": 2368700635, "price": 4092.08},
        {"leg": 2, "order": 2368700636, "price": 4092.08},
        {"leg": 3, "order": 2368700638, "price": 4092.08},
    ]}
    return sized, envoi


def test_le_stop_et_les_cibles_sont_ecrits_des_lenvoi():
    # Le 09/11, une jambe a vécu et est morte entre deux tours du watcher.
    # MT5 ne conservant pas le stop d'une position fermée, son R est devenu
    # incalculable : rien ne reconstitue après coup ce qui n'a jamais été
    # écrit. Ici le stop est connu, c'est celui qu'on vient de transmettre.
    sized, envoi = _plan_et_envoi()
    lot = journal_module.from_execution(_Compte(), sized, envoi, 760430423)

    assert all(t["stop_loss"] == 4084.30 for t in lot)
    assert [t["take_profit"] for t in lot] == [4096.30, 4100.30, 4104.30]
    assert {t["plan_id"] for t in lot} == {"760430423"}
    assert {t["symbol"] for t in lot} == {"XAUUSD"}
    assert [t["lot_size"] for t in lot] == [0.42, 0.42, 0.44]


def test_le_ticket_de_position_est_celui_de_lordre():
    # Sur un ordre au marché, MT5 donne à la position l'identifiant de l'ordre
    # qui l'a ouverte. C'est ce qui permet au watcher de retrouver plus tard
    # la ligne créée ici, et donc de la compléter au lieu de la dupliquer.
    sized, envoi = _plan_et_envoi()
    lot = journal_module.from_execution(_Compte(), sized, envoi, 1)
    assert [t["ticket"] for t in lot] == ["2368700635", "2368700636", "2368700638"]


def test_une_jambe_sans_correspondance_est_ignoree():
    # Si l'envoi s'est arrêté à mi-chemin, seules les jambes réellement
    # passées sont enregistrées.
    sized, _ = _plan_et_envoi()
    partiel = {"legs": [{"leg": 1, "order": 2368700635, "price": 4092.08}]}
    lot = journal_module.from_execution(_Compte(), sized, partiel, 1)
    assert len(lot) == 1
    assert lot[0]["take_profit"] == 4096.30


def test_un_prix_de_remplissage_absent_retombe_sur_la_reference():
    sized, _ = _plan_et_envoi()
    sans_prix = {"legs": [{"leg": 1, "order": 1, "price": 0.0}]}
    lot = journal_module.from_execution(_Compte(), sized, sans_prix, 1)
    assert lot[0]["open_price"] == sized.reference_price


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
