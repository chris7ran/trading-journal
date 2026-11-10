"""Tests de lecture des signaux, sur les messages réels des trois émetteurs.

    py -3.12 test_signals.py

Les messages sont recopiés **tels qu'ils arrivent**, emojis compris. Un
analyseur éprouvé sur des exemples nettoyés échoue au premier vrai message, et
il échoue en silence : il rend un plan, simplement pas le bon.
"""

from __future__ import annotations

from signals import LONG, SHORT, Signal, SignalError, parse

SCALE_FX = """3🔼XAU/USD BUY 🔼

Entry: 4053 - 4049

SL: 4045

Targets ✅
TP 1: 4057
TP 2: 4061
TP 3: 4065

🧠 Important: This is not financial advice. Use proper risk management and choose your lot size based on your capital.

📌 Recommended lot size"""

FX_CLUB = """FX CLUB

👉🏻 SELL XAUUSD 📉
Entrée : 4096

TP1 ➡️ 4065
TP2 ➡️ 4033

SL : 4111

RISQUE 1R"""

GOLDMIND = """🔴 SELL NASDAQ

📉 PE : 26850

🛑 SL : 26960 -110

🎯 TP1 : 26750
🎯 TP2 : 26530
🎯 TP3 : 26230

⚠️ MONEY MANAGEMENT
Risque maximum : 1 % du capital.
Attention au décalage de cotation entre ce que je vous donne et ceux que vous avez chez votre broker.

☑️ À TP1 : Half + BE obligatoire."""


# --- Scale FX -------------------------------------------------------------------


def test_scale_fx_est_lu_entierement():
    s = parse(SCALE_FX)
    assert s.symbol == "XAUUSD"
    assert s.raw_symbol == "XAU/USD"
    assert s.direction == LONG
    assert s.entry_range == (4053.0, 4049.0)
    assert s.stop_loss == 4045.0
    assert s.targets == [4057.0, 4061.0, 4065.0]


def test_la_fourchette_garde_lordre_decriture():
    # Scale FX écrit du plus haut au plus bas pour un achat, et c'est le
    # premier nombre qu'ils enregistrent dans leur propre journal. Trier
    # effacerait l'information.
    s = parse(SCALE_FX)
    assert s.entry_range[0] == 4053.0
    assert s.entry_first == 4053.0
    assert s.entry_best == 4049.0


def test_le_premier_bord_reproduit_le_ratio_quils_annoncent():
    # Leur guide dit « all 3 TP = around +1R ». Depuis 4053 le stop fait 8 $
    # et les cibles +4 / +8 / +12, soit des R de 0,5 / 1,0 / 1,5 dont la
    # moyenne vaut exactement 1,0. Depuis 4049 elle vaudrait 3,0.
    s = parse(SCALE_FX)
    risque = s.entry_first - s.stop_loss
    r = [(cible - s.entry_first) / risque for cible in s.targets]
    assert r == [0.5, 1.0, 1.5]
    assert sum(r) / 3 == 1.0

    risque_favorable = s.entry_best - s.stop_loss
    r_favorable = [(c - s.entry_best) / risque_favorable for c in s.targets]
    assert sum(r_favorable) / 3 == 3.0


def test_le_numero_de_signal_nest_pas_pris_pour_un_prix():
    # Le message commence par « 3🔼XAU/USD BUY ».
    s = parse(SCALE_FX)
    assert 3.0 not in s.targets
    assert s.stop_loss == 4045.0


def test_les_cibles_espacees_sont_reconnues():
    # « TP 1: » avec une espace, contre « TP1 : » ailleurs.
    assert len(parse(SCALE_FX).targets) == 3


# --- FX CLUB --------------------------------------------------------------------


def test_fx_club_est_lu_malgre_ses_libelles_francais():
    s = parse(FX_CLUB)
    assert s.symbol == "XAUUSD"
    assert s.direction == SHORT
    assert s.entry == 4096.0
    assert s.entry_range is None
    assert s.stop_loss == 4111.0
    assert s.targets == [4065.0, 4033.0]


def test_le_stop_place_apres_les_cibles_est_quand_meme_trouve():
    # Chez FX CLUB le SL vient en dernier, après les TP.
    assert parse(FX_CLUB).stop_loss == 4111.0


def test_deux_cibles_seulement_est_un_signal_valide():
    # Tous les émetteurs n'en donnent pas trois ; le découpage s'adapte.
    assert len(parse(FX_CLUB).targets) == 2


def test_un_prix_unique_sert_des_deux_cotes():
    s = parse(FX_CLUB)
    assert s.entry_first == 4096.0
    assert s.entry_best == 4096.0


# --- goldmind -------------------------------------------------------------------


def test_goldmind_est_lu_avec_son_vocabulaire():
    s = parse(GOLDMIND)
    assert s.symbol == "NAS100"
    assert s.raw_symbol == "NASDAQ"
    assert s.direction == SHORT
    assert s.entry == 26850.0
    assert s.targets == [26750.0, 26530.0, 26230.0]


def test_la_distance_collee_au_stop_nest_pas_prise_pour_le_stop():
    # « SL : 26960 -110 » : le second nombre est la distance en points.
    # Prendre autre chose que le premier donnerait un stop à 110.
    assert parse(GOLDMIND).stop_loss == 26960.0


def test_une_distance_collee_nest_pas_non_plus_une_fourchette():
    # Le tiret de « 26960 -110 » colle au nombre ; celui de « 4053 - 4049 »
    # est entouré d'espaces. C'est ce qui les distingue.
    assert parse(GOLDMIND).entry_range is None


def test_les_consignes_de_gestion_sont_relevees_sans_etre_appliquees():
    # « Half + BE » décrit deux jambes à 50/50, pas trois tiers. Le module
    # le signale et laisse le trader décider.
    notes = parse(GOLDMIND).notes
    assert any("moitié" in n for n in notes)
    assert any("1" in n and "%" in n for n in notes)


def test_le_risque_conseille_de_fx_club_est_releve():
    assert any("R" in n for n in parse(FX_CLUB).notes)


# --- Refus ----------------------------------------------------------------------


def test_un_message_sans_sens_est_refuse():
    try:
        parse("XAUUSD\nEntry: 4050\nSL: 4045\nTP1: 4060")
    except SignalError as e:
        assert "sens" in str(e)
    else:
        raise AssertionError("un message sans achat ni vente doit être refusé")


def test_un_message_sans_instrument_est_refuse():
    try:
        parse("BUY\nEntry: 4050\nSL: 4045\nTP1: 4060")
    except SignalError as e:
        assert "instrument" in str(e)
    else:
        raise AssertionError("un message sans instrument doit être refusé")


def test_un_message_sans_stop_est_refuse():
    # Le pire des cas : un plan sans stop est une position sans filet, et
    # inventer un stop par défaut serait inventer le risque.
    try:
        parse("BUY XAUUSD\nEntry: 4050\nTP1: 4060")
    except SignalError as e:
        assert "stop" in str(e)
    else:
        raise AssertionError("un message sans stop doit être refusé")


def test_un_message_sans_cible_est_refuse():
    try:
        parse("BUY XAUUSD\nEntry: 4050\nSL: 4045")
    except SignalError as e:
        assert "cible" in str(e)
    else:
        raise AssertionError("un message sans cible doit être refusé")


def test_un_message_vide_est_refuse():
    try:
        parse("   ")
    except SignalError:
        pass
    else:
        raise AssertionError("un message vide doit être refusé")


# --- Tolérance de forme ---------------------------------------------------------


def test_les_decimales_a_la_virgule_sont_lues():
    s = parse("BUY XAUUSD\nEntry: 4050,50\nSL: 4045,25\nTP1: 4060,75")
    assert s.entry == 4050.50
    assert s.stop_loss == 4045.25
    assert s.targets == [4060.75]


def test_les_milliers_espaces_sont_lus():
    s = parse("SELL NASDAQ\nPE : 26 850\nSL : 26 960\nTP1 : 26 750")
    assert s.entry == 26850.0
    assert s.stop_loss == 26960.0


def test_les_cibles_sont_ordonnees_par_numero_pas_par_position():
    s = parse("BUY XAUUSD\nEntry: 4050\nSL: 4045\nTP3: 4075\nTP1: 4055\nTP2: 4065")
    assert s.targets == [4055.0, 4065.0, 4075.0]


def test_le_mot_or_ne_declenche_pas_un_faux_instrument():
    # « OR » est un alias de l'or, et un mot français courant. Il ne doit pas
    # suffire à lui seul quand un vrai instrument est présent.
    s = parse("SELL NASDAQ\nPE : 26850\nSL : 26960\nTP1 : 26750\nor donc")
    assert s.symbol == "NAS100"

# --- Deux formats de plus, trouvés à l'usage ------------------------------------

GOLD_NUMEROTE = """📈 BUY GOLD – XAUUSD
🟢 Entrée : 4116.50
🛑 SL : 4101.50
🎯 TP1 : 4126.50
🎯 TP2 : 4136.50
🎯 TP3 : 4146.50
⚠️ Risque maximum : 1 % du capital
☑️ À TP1 : Half Close + Break Even (BE)"""

BTC_GROUPE = """📉️ SELL BTCUSD

👉️ PE 74400-500
🛑️ SL 74600

✅ TP 74170 / 73500 / 73000

⚠️ Money Management
Risque maximum : 1 % du capital."""


def test_le_symbole_ecrit_deux_fois_ne_gene_pas():
    # « BUY GOLD – XAUUSD » nomme l'or de deux façons sur la même ligne.
    s = parse(GOLD_NUMEROTE)
    assert s.symbol == "XAUUSD"
    assert s.direction == LONG
    assert s.entry == 4116.50
    assert s.targets == [4126.50, 4136.50, 4146.50]


def test_une_consigne_de_gestion_a_la_fin_ne_cree_pas_de_fausse_cible():
    # « À TP1 : Half Close + Break Even (BE) » porte le motif d'une cible mais
    # aucun prix. La ligne ne doit pas écraser la vraie TP1.
    assert parse(GOLD_NUMEROTE).targets[0] == 4126.50


def test_half_close_plus_break_even_est_reconnu():
    # Les mots qui s'intercalent varient ; la paire moitié / point mort, non.
    assert any("moitié" in n for n in parse(GOLD_NUMEROTE).notes)


def test_des_cibles_listees_sur_une_seule_ligne_sont_lues_dans_lordre():
    # « TP 74170 / 73500 / 73000 » : aucun numéro, l'ordre d'écriture fait foi.
    s = parse(BTC_GROUPE)
    assert s.targets == [74170.0, 73500.0, 73000.0]


def test_un_prix_ne_se_fait_pas_passer_pour_un_numero_de_cible():
    # Sans frontière de mot après le numéro, « TP 74170 » se lirait « TP 7 »
    # suivi du prix 4170 : une cible inventée, plausible, et fausse de deux
    # ordres de grandeur.
    assert 4170.0 not in parse(BTC_GROUPE).targets


def test_une_fourchette_abregee_retient_le_premier_prix():
    # « PE 74400-500 » abrège 74400–74500. Le tiret colle aux chiffres, donc
    # ce n'est pas lu comme une fourchette — et le premier nombre est de toute
    # façon la référence retenue.
    assert parse(BTC_GROUPE).entry == 74400.0


def test_les_cryptos_sont_reconnues():
    assert parse(BTC_GROUPE).symbol == "BTCUSD"


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
