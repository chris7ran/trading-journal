"""Tests des envois et du journal d'audit, sans terminal.

Un faux MT5 suffit : ce qui doit être vérifié ici n'est pas que le broker
répond, c'est que la **requête formée est la bonne**. Le mode d'exécution, le
type d'ordre déduit, la présence du stop dès l'entrée, l'heure d'expiration —
tout cela se décide dans ce module et nulle part ailleurs.

    py -3.12 test_orders.py
"""

from __future__ import annotations

import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace

import audit
import orders
from orders import OrderError


class FauxMT5:
    """Le strict nécessaire pour former et juger une requête."""

    def __init__(self, filling_mode: int = 2, bid: float = 4110.40, ask: float = 4110.73):
        self.filling_mode = filling_mode
        self.bid, self.ask = bid, ask
        self.envoye: list[dict] = []
        self.retcode = orders.RETCODE_DONE

    def symbol_info(self, symbol):
        return SimpleNamespace(filling_mode=self.filling_mode)

    def symbol_info_tick(self, symbol):
        return SimpleNamespace(bid=self.bid, ask=self.ask)

    def order_send(self, request):
        self.envoye.append(request)
        return SimpleNamespace(
            retcode=self.retcode, comment="ok", order=1, deal=2,
            volume=request.get("volume", 0.0), price=request.get("price", 0.0),
            request_id=7,
        )

    def order_check(self, request):
        return SimpleNamespace(
            retcode=0, comment="ok", margin=450.0, margin_free=99_550.0, balance=100_000.0
        )

    def last_error(self):
        return (0, "")


def jambe(mt5, **kwargs):
    base = dict(
        symbol="GOLD#", direction="LONG", volume=0.41,
        stop_loss=4102.70, take_profit=4114.70,
        magic=123456789, comment="plan-abc",
    )
    base.update(kwargs)
    return orders.market_leg(mt5, **base)


# --- Le piège des deux numérotations ------------------------------------------


def test_le_masque_du_symbole_nest_pas_la_valeur_de_la_requete():
    # Chez XM, filling_mode vaut 2 (IOC autorisé), et la requête doit porter
    # 1 (ORDER_FILLING_IOC). Recopier le masque enverrait du FOK, que le
    # serveur rejette sans expliquer.
    mt5 = FauxMT5(filling_mode=2)
    assert orders.filling_for(mt5, "GOLD#") == orders.ORDER_FILLING_IOC
    assert orders.ORDER_FILLING_IOC == 1
    assert orders.FILLING_FLAG_IOC == 2
    assert orders.ORDER_FILLING_IOC != orders.FILLING_FLAG_IOC


def test_fok_quand_cest_le_seul_mode_autorise():
    mt5 = FauxMT5(filling_mode=1)
    assert orders.filling_for(mt5, "GOLD#") == orders.ORDER_FILLING_FOK


def test_le_mode_dexecution_part_bien_dans_la_requete():
    mt5 = FauxMT5(filling_mode=2)
    jambe(mt5)
    assert mt5.envoye[0]["type_filling"] == orders.ORDER_FILLING_IOC


# --- Ordre au marché ----------------------------------------------------------


def test_le_stop_part_avec_lentree_pas_apres():
    # Une position ouverte une seconde sans stop est une position sans filet
    # si le processus meurt entre les deux appels.
    mt5 = FauxMT5()
    jambe(mt5)
    envoi = mt5.envoye[0]
    assert envoi["sl"] == 4102.70
    assert envoi["tp"] == 4114.70
    assert envoi["action"] == orders.TRADE_ACTION_DEAL


def test_un_achat_part_a_lask_une_vente_au_bid():
    mt5 = FauxMT5(bid=4110.40, ask=4110.73)
    jambe(mt5)
    assert mt5.envoye[0]["price"] == 4110.73
    jambe(mt5, direction="SHORT", stop_loss=4118.0, take_profit=4106.0)
    assert mt5.envoye[1]["price"] == 4110.40
    assert mt5.envoye[1]["type"] == orders.ORDER_TYPE_SELL


def test_le_glissement_accepte_est_borne():
    mt5 = FauxMT5()
    jambe(mt5)
    assert mt5.envoye[0]["deviation"] == orders.DEFAULT_DEVIATION


def test_le_commentaire_est_tronque_a_trente_et_un_caracteres():
    # Au-delà, MT5 coupe lui-même ; mieux vaut le faire sciemment.
    mt5 = FauxMT5()
    jambe(mt5, comment="x" * 60)
    assert len(mt5.envoye[0]["comment"]) == 31


def test_le_magic_identifie_le_plan():
    mt5 = FauxMT5()
    jambe(mt5, magic=987654321)
    assert mt5.envoye[0]["magic"] == 987654321


def test_un_refus_du_broker_leve_plutot_que_de_passer_inapercu():
    mt5 = FauxMT5()
    mt5.retcode = 10019  # fonds insuffisants
    try:
        jambe(mt5)
    except OrderError as e:
        assert "10019" in str(e)
    else:
        raise AssertionError("un retcode d'échec doit lever")


def test_sans_cotation_rien_nest_envoye():
    mt5 = FauxMT5(bid=0.0, ask=0.0)
    try:
        jambe(mt5)
    except OrderError:
        assert mt5.envoye == []
    else:
        raise AssertionError("un prix nul doit lever avant l'envoi")


# --- Ordre en attente ---------------------------------------------------------


def en_attente(mt5, entry, **kwargs):
    base = dict(
        symbol="GOLD#", direction="LONG", volume=0.41, entry=entry,
        stop_loss=4102.70, take_profit=4114.70, magic=1, comment="p",
        expires_at_server=datetime(2026, 11, 9, 9, 0, 0),
    )
    base.update(kwargs)
    return orders.pending_leg(mt5, **base)


def test_un_achat_sous_le_marche_est_une_limite():
    mt5 = FauxMT5(ask=4110.73)
    en_attente(mt5, entry=4108.00)
    assert mt5.envoye[0]["type"] == orders.ORDER_TYPE_BUY_LIMIT


def test_un_achat_au_dessus_du_marche_est_un_stop():
    mt5 = FauxMT5(ask=4110.73)
    en_attente(mt5, entry=4113.00)
    assert mt5.envoye[0]["type"] == orders.ORDER_TYPE_BUY_STOP


def test_une_vente_au_dessus_du_marche_est_une_limite():
    mt5 = FauxMT5(bid=4110.40)
    en_attente(mt5, entry=4113.00, direction="SHORT",
               stop_loss=4120.0, take_profit=4105.0)
    assert mt5.envoye[0]["type"] == orders.ORDER_TYPE_SELL_LIMIT


def test_une_expiration_est_toujours_posee():
    mt5 = FauxMT5()
    en_attente(mt5, entry=4108.00)
    envoi = mt5.envoye[0]
    assert envoi["type_time"] == orders.ORDER_TIME_SPECIFIED
    assert envoi["expiration"] == datetime(2026, 11, 9, 9, 0, 0)


def test_lexpiration_est_convertie_en_heure_serveur():
    # Chez XM le serveur est à UTC+2. Une expiration posée naïvement en UTC
    # annulerait l'ordre deux heures trop tôt — donc avant d'avoir existé.
    avant = datetime.now(timezone.utc).replace(tzinfo=None)
    echeance = orders.expiry_in_server_time(30, 7200)
    ecart = (echeance - avant).total_seconds()
    assert 2 * 3600 + 29 * 60 < ecart < 2 * 3600 + 31 * 60


def test_sans_decalage_mesure_aucune_expiration_nest_datee():
    try:
        orders.expiry_in_server_time(30, None)
    except OrderError as e:
        assert "décalage" in str(e)
    else:
        raise AssertionError("dater une expiration sans horloge doit lever")


def test_un_ordre_pose_mais_non_rempli_est_un_succes():
    # 10008 est la réponse normale d'un ordre en attente accepté.
    mt5 = FauxMT5()
    mt5.retcode = orders.RETCODE_PLACED
    resultat = en_attente(mt5, entry=4108.00)
    assert resultat["retcode"] == orders.RETCODE_PLACED


def test_lannulation_vise_un_ticket():
    mt5 = FauxMT5()
    orders.cancel_order(mt5, 55512345)
    assert mt5.envoye[0] == {"action": orders.TRADE_ACTION_REMOVE, "order": 55512345}


# --- Journal d'audit ----------------------------------------------------------


def journal():
    return audit.Audit(Path(tempfile.mkdtemp()), "xm-demo")


def test_le_demarrage_ecrit_une_ligne():
    # Vérifier le disque tant qu'aucun ordre n'en dépend : découvrir que F:
    # est plein au moment d'envoyer serait le découvrir trop tard.
    j = journal()
    assert j.read_day()[0]["event"] == "bridge_started"


def test_chaque_ligne_porte_son_horodatage_et_son_compte():
    j = journal()
    j.write("order_intent", {"volume": 0.41}, plan_id="abc")
    ligne = j.read_day()[-1]
    assert ligne["account"] == "xm-demo"
    assert ligne["plan_id"] == "abc"
    assert ligne["at"].endswith("Z")


def test_rien_nest_jamais_reecrit():
    j = journal()
    for i in range(5):
        j.write("order_intent", {"i": i}, plan_id=str(i))
    lignes = [l for l in j.read_day() if l["event"] == "order_intent"]
    assert [l["payload"]["i"] for l in lignes] == [0, 1, 2, 3, 4]


def test_une_intention_sans_resultat_est_signalee():
    # Le signal le plus précieux du fichier : un ordre parti sans qu'on sache
    # ce qu'il est devenu.
    j = journal()
    j.write("order_intent", {"leg": 1}, plan_id="complet")
    j.write("order_result", {"ticket": 1}, plan_id="complet")
    j.write("order_intent", {"leg": 1}, plan_id="orphelin")
    orphelins = j.orphans()
    assert [o["plan_id"] for o in orphelins] == ["orphelin"]


def test_un_echec_declare_nest_pas_un_orphelin():
    j = journal()
    j.write("order_intent", {"leg": 1}, plan_id="refuse")
    j.write("order_failed", {"retcode": 10019}, plan_id="refuse")
    assert j.orphans() == []


def test_un_journal_inaccessible_leve_au_lieu_de_se_taire():
    # Pas de trace, pas d'ordre : l'appelant doit renoncer à envoyer.
    try:
        audit.Audit(Path("/dev/null/impossible"), "xm-demo")
    except audit.AuditError as e:
        assert "audit" in str(e).lower()
    else:
        raise AssertionError("un chemin inécrivable doit lever")


def test_une_ligne_tronquee_ninvalide_pas_le_fichier():
    j = journal()
    j.write("order_intent", {"ok": True}, plan_id="a")
    with j.path.open("a", encoding="utf-8") as fh:
        fh.write('{"at": "2026-11-09T0')  # coupure en pleine écriture
    lignes = j.read_day()
    assert lignes[-1]["event"] == "ligne_illisible"
    assert any(l["event"] == "order_intent" for l in lignes)


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
