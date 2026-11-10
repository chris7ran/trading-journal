"""Tests du bot : routage des boutons quand plusieurs comptes sont branchés.

    py -3.12 test_bot.py

Rien ici ne parle à Telegram ni au pont. Un faux client rend des aperçus
écrits à la main, et un faux transport retient ce que le bot aurait envoyé —
ce qui laisse tester la seule chose qui compte vraiment ici : qu'un appui sur
le bouton d'un compte agisse sur ce compte et pas sur un autre.
"""

from __future__ import annotations

import bot as botmod
import mcpclient
import signals

SIGNAL = """👉🏻 SELL XAUUSD 📉
Entrée : 4096
TP1 ➡️ 4065
TP2 ➡️ 4033
SL : 4111"""


def _apercu(alias: str, decision: str = "accepté", volume: float = 1.0) -> dict:
    return {
        "account": alias,
        "decision": decision,
        "findings": [],
        "plan": {
            "symbol": "GOLD#", "journal_symbol": "XAUUSD", "direction": "SHORT",
            "reference_price": 4096.0, "stop_loss": 4111.0, "stop_distance": 15.0,
            "total_volume": volume, "risk_amount": 1000.0, "risk_pct": 0.01,
            "currency": "USD", "blended_r": 1.5, "zone": "green",
            "legs": [{"index": 1, "volume": volume, "take_profit": 4065.0,
                      "r_multiple": 2.0}],
        },
    }


class FauxClient:
    """Rend ce qu'on lui dit de rendre, et note tout ce qu'on lui demande."""

    def __init__(self, reponses: dict) -> None:
        self.reponses = reponses
        self.appels: list[tuple[str, dict]] = []

    def call(self, nom: str, arguments: dict | None = None) -> dict:
        arguments = arguments or {}
        self.appels.append((nom, arguments))
        valeur = self.reponses.get(nom)
        if isinstance(valeur, Exception):
            raise valeur
        if callable(valeur):
            return valeur(arguments)
        if valeur is None:
            raise AssertionError(f"appel non prévu dans ce test : {nom}")
        return valeur


class FauxBot(botmod.Bot):
    """Un bot dont les envois Telegram restent en mémoire."""

    def __init__(self, client, comptes) -> None:
        super().__init__("jeton-bidon", 42, client, comptes, None)
        self.envois: list[tuple[str, dict | None]] = []

    def _envoyer(self, chat, texte, boutons=None):  # type: ignore[override]
        self.envois.append((texte, boutons))

    def _api(self, methode, parametres, timeout=30):  # type: ignore[override]
        return {"ok": True}


def _callbacks(boutons: dict) -> list[str]:
    return [b["callback_data"] for rangee in boutons["inline_keyboard"] for b in rangee]


# --- Un seul compte : rien ne doit changer --------------------------------------


def test_un_seul_compte_garde_la_presentation_detaillee():
    client = FauxClient({"preview_plan": _apercu("xm-demo")})
    b = FauxBot(client, ["xm-demo"])
    b._chiffrer(1, SIGNAL)

    texte, boutons = b.envois[-1]
    assert "R pondéré" in texte  # le rendu détaillé, pas la synthèse
    assert "Exécuter" in str(boutons)


def test_un_seul_compte_route_quand_meme_vers_lui():
    client = FauxClient({
        "preview_plan": _apercu("xm-demo"),
        "execute_plan": {"sent": True, "execution": {"magic": 1, "legs": [], "mode": "market"}},
    })
    b = FauxBot(client, ["xm-demo"])
    b._chiffrer(1, SIGNAL)
    jeton = next(iter(b.propositions))

    b._bouton({"id": "1", "from": {"id": 42}, "message": {"chat": {"id": 1}},
               "data": f"x:{jeton}:0"})
    envoi = [a for a in client.appels if a[0] == "execute_plan"][0]
    assert envoi[1]["account"] == "xm-demo"


# --- Plusieurs comptes ----------------------------------------------------------


def test_chaque_compte_est_chiffre_separement():
    client = FauxClient({
        "preview_plan": lambda a: _apercu(a["account"], volume=2.0 if a["account"] == "b" else 1.0)
    })
    b = FauxBot(client, ["a", "b"])
    b._chiffrer(1, SIGNAL)

    demandes = [a[1]["account"] for a in client.appels if a[0] == "preview_plan"]
    assert demandes == ["a", "b"]


def test_un_bouton_designe_son_compte():
    # Le cœur du sujet : le second bouton doit envoyer sur le second compte.
    client = FauxClient({
        "preview_plan": lambda a: _apercu(a["account"]),
        "execute_plan": {"sent": True, "execution": {"magic": 7, "legs": [], "mode": "market"}},
    })
    b = FauxBot(client, ["xm-demo", "autre-broker"])
    b._chiffrer(1, SIGNAL)
    jeton = next(iter(b.propositions))

    b._bouton({"id": "1", "from": {"id": 42}, "message": {"chat": {"id": 1}},
               "data": f"x:{jeton}:1"})
    envoi = [a for a in client.appels if a[0] == "execute_plan"][0]
    assert envoi[1]["account"] == "autre-broker"


def test_lenvoi_sur_un_compte_laisse_lautre_disponible():
    # Si la proposition disparaissait au premier succès, le choix offert
    # n'en serait pas un : le second compte deviendrait inatteignable.
    client = FauxClient({
        "preview_plan": lambda a: _apercu(a["account"]),
        "execute_plan": {"sent": True, "execution": {"magic": 7, "legs": [], "mode": "market"}},
    })
    b = FauxBot(client, ["a", "b"])
    b._chiffrer(1, SIGNAL)
    jeton = next(iter(b.propositions))

    b._bouton({"id": "1", "from": {"id": 42}, "message": {"chat": {"id": 1}},
               "data": f"x:{jeton}:0"})
    assert jeton in b.propositions

    b._bouton({"id": "2", "from": {"id": 42}, "message": {"chat": {"id": 1}},
               "data": f"x:{jeton}:1"})
    envois = [a[1]["account"] for a in client.appels if a[0] == "execute_plan"]
    assert envois == ["a", "b"]


def test_un_second_appui_sur_le_meme_compte_est_refuse():
    client = FauxClient({
        "preview_plan": lambda a: _apercu(a["account"]),
        "execute_plan": {"sent": True, "execution": {"magic": 7, "legs": [], "mode": "market"}},
    })
    b = FauxBot(client, ["a", "b"])
    b._chiffrer(1, SIGNAL)
    jeton = next(iter(b.propositions))

    for _ in range(2):
        b._bouton({"id": "1", "from": {"id": 42}, "message": {"chat": {"id": 1}},
                   "data": f"x:{jeton}:0"})

    envois = [a for a in client.appels if a[0] == "execute_plan"]
    assert len(envois) == 1
    assert "Déjà envoyé" in b.envois[-1][0]


def test_un_compte_injoignable_nempeche_pas_les_autres():
    def reponse(arguments):
        if arguments["account"] == "mort":
            raise mcpclient.McpError("lien rompu avec le terminal")
        return _apercu(arguments["account"])

    client = FauxClient({"preview_plan": reponse})
    b = FauxBot(client, ["mort", "vivant"])
    b._chiffrer(1, SIGNAL)

    texte, boutons = b.envois[-1]
    assert "chiffrage impossible" in texte
    # Aucun bouton pour le compte mort, mais ceux du compte vivant sont là.
    assert all(":0" not in c or c.startswith("n:") for c in _callbacks(boutons))
    assert any(c.endswith(":1") for c in _callbacks(boutons))


def test_un_compte_refuse_garde_le_depot_et_perd_lexecution():
    def reponse(arguments):
        if arguments["account"] == "plein":
            a = _apercu("plein", decision="refusé")
            a["findings"] = [{"level": "block", "code": "too_many_plans",
                              "message": "1 plan(s) déjà ouvert(s) pour un maximum de 1"}]
            return a
        return _apercu(arguments["account"])

    client = FauxClient({"preview_plan": reponse})
    b = FauxBot(client, ["plein", "libre"])
    b._chiffrer(1, SIGNAL)

    texte, boutons = b.envois[-1]
    assert "1 plan(s) déjà ouvert(s)" in texte
    codes = _callbacks(boutons)
    assert "a:" in " ".join(c[:2] + ":" for c in codes)
    assert not any(c.startswith("x:") and c.endswith(":0") for c in codes)
    assert any(c.startswith("x:") and c.endswith(":1") for c in codes)


def test_la_fermeture_vise_le_compte_de_son_bouton():
    positions = {"count": 1, "positions": [{
        "journal_symbol": "XAUUSD", "direction": "SHORT", "volume": 0.5,
        "open_price": 4096.0, "profit": 12.0, "magic": 0,
    }]}
    client = FauxClient({
        "get_positions": positions,
        "close_positions": {"closed": [], "realised": 0.0},
    })
    b = FauxBot(client, ["a", "b"])
    b._fermer(1)
    jeton = next(iter(b.propositions))

    b._bouton({"id": "1", "from": {"id": 42}, "message": {"chat": {"id": 1}},
               "data": f"f:{jeton}:1"})
    fermeture = [a for a in client.appels if a[0] == "close_positions"][0]
    assert fermeture[1]["account"] == "b"


def test_aucun_bouton_ne_ferme_tous_les_comptes_a_la_fois():
    positions = {"count": 1, "positions": [{
        "journal_symbol": "XAUUSD", "direction": "SHORT", "volume": 0.5,
        "open_price": 4096.0, "profit": 12.0, "magic": 0,
    }]}
    client = FauxClient({"get_positions": positions})
    b = FauxBot(client, ["a", "b"])
    b._fermer(1)

    _, boutons = b.envois[-1]
    fermetures = [c for c in _callbacks(boutons) if c.startswith("f:")]
    assert len(fermetures) == 2  # un par compte, jamais un pour les deux


def test_letat_survit_a_un_compte_muet():
    def reponse(nom):
        def _inner(arguments):
            if arguments["account"] == "mort":
                raise mcpclient.McpError("terminal arrêté")
            if nom == "get_account":
                return {"balance": 1000.0, "equity": 1000.0, "margin_free": 900.0,
                        "currency": "USD"}
            return {"count": 0, "positions": []}
        return _inner

    client = FauxClient({"get_account": reponse("get_account"),
                         "get_positions": reponse("get_positions")})
    b = FauxBot(client, ["mort", "vivant"])
    b._etat(1)

    texte = b.envois[-1][0]
    assert "injoignable" in texte
    assert "vivant" in texte


# --- Garde-fous de forme --------------------------------------------------------


def test_le_callback_tient_dans_la_limite_de_telegram():
    # Telegram plafonne `callback_data` à 64 octets. Le jeton fait 11
    # caractères, l'action 1, le rang 1 ou 2 : large, mais c'est le genre de
    # limite qu'on découvre en production.
    client = FauxClient({"preview_plan": lambda a: _apercu(a["account"])})
    b = FauxBot(client, [f"compte-{i}" for i in range(10)])
    b._chiffrer(1, SIGNAL)

    for code in _callbacks(b.envois[-1][1]):
        assert len(code.encode()) <= 64, code


def test_le_signal_nest_ecrit_quune_fois():
    client = FauxClient({"preview_plan": lambda a: _apercu(a["account"])})
    b = FauxBot(client, ["a", "b", "c"])
    b._chiffrer(1, SIGNAL)

    texte = b.envois[-1][0]
    assert texte.count("stop 4111") == 1


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
