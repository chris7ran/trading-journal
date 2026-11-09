"""Tests du dimensionnement.

Sans dépendance : `py -3.12 test_plan.py` suffit, et pytest les ramasse aussi
s'il est installé. Ce module est le seul du pont dont une erreur se paie en
argent, donc il est testé plus que le reste.
"""

from __future__ import annotations

import plan
from plan import PlanError, SymbolSpec, Thresholds

# Relevés sur le compte XM le 08/11/2026, pas inventés.
GER40 = SymbolSpec(
    name="GER40Cash#",
    digits=2,
    point=0.01,
    tick_size=0.01,
    tick_value=0.01,
    contract_size=1.0,
    volume_min=0.1,
    volume_step=0.1,
    volume_max=150.0,
)

GOLD = SymbolSpec(
    name="GOLD#",
    digits=2,
    point=0.01,
    tick_size=0.01,
    tick_value=1.0,
    contract_size=100.0,
    volume_min=0.01,
    volume_step=0.01,
    volume_max=50.0,
)


def build(**kwargs):
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


# --- Valeur du point ---------------------------------------------------------


def test_un_point_dindice_vaut_un_dollar_par_lot():
    assert GER40.value_per_price_unit == 1.0


def test_un_dollar_dor_vaut_cent_dollars_par_lot():
    # contract_size 100 onces : le tick de 0,01 vaut 1 USD, donc 1 USD de
    # variation en vaut 100. Supposer la même valeur que sur un indice
    # ouvrirait une position cent fois trop grosse.
    assert GOLD.value_per_price_unit == 100.0


# --- Dimensionnement ---------------------------------------------------------


def test_un_pourcent_sur_cent_mille_avec_un_stop_de_cinquante_points():
    p = build()
    assert p.total_volume == 20.0
    assert p.risk_amount == 1_000.0
    assert round(p.risk_pct, 6) == 0.01


def test_lor_se_dimensionne_sur_son_propre_pas():
    p = build(
        spec=GOLD,
        stop_loss=3_995.0,
        targets=[4_005.0, 4_010.0, 4_015.0],
        market_price=4_000.0,
    )
    # 1000 USD risqués sur 5 USD de stop à 100 USD le dollar : 2 lots.
    assert p.total_volume == 2.0
    assert [leg.volume for leg in p.legs] == [0.66, 0.66, 0.68]


def test_larrondi_se_fait_toujours_vers_le_bas():
    # 1000 / (49 × 1) = 20,408 lot. Arrondir au supérieur dépasserait le
    # risque explicitement demandé.
    p = build(stop_loss=26_251.0)
    assert p.total_volume == 20.4
    assert p.risk_amount < 1_000.0


def test_le_volume_plafonne_par_le_broker_est_signale():
    p = build(equity=100_000_000.0)
    assert p.total_volume == 150.0
    w = next(w for w in p.warnings if w.code == "volume_capped")
    # Demande un oui : le risque réel n'est plus celui demandé, et rien
    # d'autre dans la sortie ne le dit.
    assert w.level == "warn"


def test_un_risque_trop_petit_pour_le_minimum_est_refuse():
    try:
        build(equity=100.0)
    except PlanError as e:
        assert "minimum" in str(e)
    else:
        raise AssertionError("un volume sous le minimum broker doit être refusé")


def test_un_volume_indivisible_en_trois_jambes_est_refuse():
    # 0,2 lot sur trois cibles donnerait une jambe à zéro.
    try:
        build(equity=1_000.0, stop_loss=26_250.0)
    except PlanError as e:
        assert "jambes" in str(e) or "minimum" in str(e)
    else:
        raise AssertionError("un volume indivisible doit être refusé")


# --- Découpage ---------------------------------------------------------------


def test_les_jambes_somment_exactement_au_total():
    p = build()
    assert sum(leg.volume for leg in p.legs) == p.total_volume


def test_le_reste_va_sur_la_cible_la_plus_lointaine():
    # 20,0 lot au pas de 0,1 font 200 pas ; 200 // 3 = 66, reste 68.
    p = build()
    assert [leg.volume for leg in p.legs] == [6.6, 6.6, 6.8]


def test_une_seule_cible_donne_une_seule_jambe():
    p = build(targets=[26_400.0])
    assert len(p.legs) == 1
    assert p.legs[0].volume == 20.0


def test_les_volumes_ne_trainent_pas_de_poussiere_flottante():
    # 6,6 + 6,6 + 6,8 en virgule flottante naïve ne fait pas 20,0, et un
    # broker refuse un volume à 10^-15 près.
    p = build()
    for leg in p.legs:
        assert repr(leg.volume) == repr(round(leg.volume, 8))


# --- Ratio -------------------------------------------------------------------


def test_le_r_est_pondere_par_le_volume_pas_moyenne():
    p = build()
    # R des jambes : 1, 2, 3 ; volumes 6,6 / 6,6 / 6,8.
    assert [leg.r_multiple for leg in p.legs] == [1.0, 2.0, 3.0]
    assert p.blended_r == 2.01  # la moyenne simple donnerait 2,00
    assert p.zone == "green"


def test_le_gabarit_scale_fx_vaut_un_r_a_lentree_prevue():
    # Stop 100 points, cibles à +50 / +100 / +150 : le modèle relevé sur
    # leurs 92 signaux. R des jambes : 0,5 / 1,0 / 1,5.
    p = build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        market_price=26_300.0,
    )
    assert [leg.r_multiple for leg in p.legs] == [0.5, 1.0, 1.5]
    # 10 lot font 100 pas : 33 / 33 / 34. Le reste sur la cible la plus
    # lointaine pousse le pondéré à 1,005 là où la moyenne simple donne
    # 1,000 — toute la différence entre les deux mesures.
    assert [leg.volume for leg in p.legs] == [3.3, 3.3, 3.4]
    assert p.blended_r == 1.005
    assert p.zone == "green"


# --- Entrée tardive ----------------------------------------------------------


def scalefx(market_price: float):
    """Le gabarit Scale FX, pris à un prix de marché donné."""
    return build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry=26_300.0,
        market_price=market_price,
    )


def test_onze_points_de_retard_posent_le_ratio_sur_le_seuil():
    # (1 − u) / (1 + u) avec u = 11 % : 0,80 pile.
    p = scalefx(26_311.0)
    assert p.zone == "green"
    assert 0.79 < p.blended_r < 0.81
    assert any(w.code == "drift" for w in p.warnings)


def test_vingt_points_de_retard_font_basculer_en_zone_dalerte():
    p = scalefx(26_320.0)
    assert p.zone == "amber"
    assert p.blended_r < 0.80


def test_un_tiers_du_stop_de_retard_fait_basculer_en_refus():
    p = scalefx(26_340.0)
    assert p.zone == "red"
    assert p.blended_r < 0.50


def test_la_degradation_contre_le_plan_est_chiffree():
    p = scalefx(26_320.0)
    assert p.planned_blended_r == 1.0
    assert 0.30 < p.degradation < 0.36  # un bon tiers d'espérance perdu


def test_le_dimensionnement_part_du_prix_reel_pas_du_prix_du_signal():
    # Entrée prévue à 26 300, marché à 26 320 : le stop n'a pas bougé, donc
    # la distance réelle est de 120 points et non de 100.
    p = scalefx(26_320.0)
    assert p.stop_distance == 120.0
    assert p.reference_price == 26_320.0
    # 1000 / 120 = 8,33 -> 8,3 lot après arrondi au pas.
    assert p.total_volume == 8.3


# --- Prix passé de l'autre côté ----------------------------------------------


def test_un_prix_parti_vers_le_stop_est_signale_malgre_un_meilleur_ratio():
    # Le prix est entre l'entrée et le stop : le R s'améliore mécaniquement
    # alors que le scénario se dégrade. C'est précisément le moment où un
    # chiffre flatteur est dangereux.
    p = scalefx(26_250.0)
    assert p.blended_r > 1.0
    w = next(w for w in p.warnings if w.code == "past_entry")
    assert w.level == "warn"


def test_une_zone_dentree_traversee_vers_le_bas_declenche_le_meme_signal():
    p = build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry_range=(26_280.0, 26_320.0),
        market_price=26_240.0,
    )
    assert any(w.code == "past_entry" for w in p.warnings)


def test_dans_la_zone_dentree_aucun_avertissement():
    p = build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry_range=(26_280.0, 26_320.0),
        market_price=26_300.0,
    )
    assert p.warnings == []


def test_la_degradation_dune_zone_se_mesure_contre_le_bord_favorable():
    # Pour un achat, la zone voulait le bas : comparer au milieu
    # sous-estimerait ce que le retard a coûté.
    p = build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        entry_range=(26_280.0, 26_320.0),
        market_price=26_330.0,
    )
    assert p.planned_blended_r is not None
    # Bord bas à 26 280 : stop de 80 points, cibles à 70 / 120 / 170.
    assert round(p.planned_blended_r, 2) == 1.5


# --- Géométrie ---------------------------------------------------------------


def test_un_stop_du_mauvais_cote_est_refuse():
    try:
        build(stop_loss=26_400.0)
    except PlanError as e:
        assert "stop" in str(e)
    else:
        raise AssertionError("un stop au-dessus du prix pour un achat doit être refusé")


def test_une_cible_deja_atteinte_est_refusee():
    try:
        build(targets=[26_290.0, 26_400.0, 26_450.0])
    except PlanError as e:
        assert "cible" in str(e)
    else:
        raise AssertionError("une cible derrière le prix doit être refusée")


def test_des_cibles_dans_le_desordre_sont_refusees():
    try:
        build(targets=[26_450.0, 26_350.0, 26_400.0])
    except PlanError as e:
        assert "ordre" in str(e)
    else:
        raise AssertionError("des cibles désordonnées doivent être refusées")


def test_une_vente_est_la_symetrie_exacte_dun_achat():
    p = build(
        direction="SELL",
        stop_loss=26_350.0,
        targets=[26_250.0, 26_200.0, 26_150.0],
        market_price=26_300.0,
    )
    assert p.direction == "SHORT"
    assert p.total_volume == 20.0
    assert [leg.r_multiple for leg in p.legs] == [1.0, 2.0, 3.0]


def test_un_stop_plus_proche_que_la_distance_minimale_du_broker_est_refuse():
    serre = SymbolSpec(**{**GER40.__dict__, "stops_level": 2_000})  # 20,00 points
    try:
        build(spec=serre, stop_loss=26_295.0)
    except PlanError as e:
        assert "broker" in str(e)
    else:
        raise AssertionError("un stop sous la distance minimale doit être refusé")


# --- Garde-fous d'entrée ------------------------------------------------------


def test_sans_cotation_rien_nest_dimensionne():
    # bid à zéro le week-end : ce n'est pas un prix, c'est une absence.
    try:
        build(market_price=0.0)
    except PlanError as e:
        assert "cotation" in str(e)
    else:
        raise AssertionError("un prix nul doit être refusé")


def test_un_sens_inconnu_est_refuse():
    for mauvais in ("", "sideways", "neutre"):
        try:
            build(direction=mauvais)
        except PlanError:
            pass
        else:
            raise AssertionError(f"'{mauvais}' doit être refusé")


def test_un_risque_hors_bornes_est_refuse():
    for mauvais in (0.0, -0.01, 1.5):
        try:
            build(risk_pct=mauvais)
        except PlanError:
            pass
        else:
            raise AssertionError(f"risque {mauvais} doit être refusé")


# --- Seuils configurables -----------------------------------------------------


def test_les_seuils_se_reglent_par_compte():
    strict = Thresholds(green=1.2, red=1.0)
    p = build(thresholds=strict)
    assert p.blended_r == 2.01
    assert p.zone == "green"
    p = scalefx(26_300.0)
    assert p.blended_r == 1.005  # acceptable sous les seuils par défaut
    p = build(
        stop_loss=26_200.0,
        targets=[26_350.0, 26_400.0, 26_450.0],
        thresholds=strict,
    )
    assert p.zone == "amber"  # 1,0 passe sous un seuil vert à 1,2


# --- Niveau des avertissements ------------------------------------------------


def test_une_simple_derive_de_prix_ninterrompt_personne():
    # Le prix bouge toujours entre la lecture d'une idée et son chiffrage.
    # Exiger une confirmation pour ça seul en exigerait une sur presque tous
    # les plans réels, et une confirmation systématique finit cliquée sans
    # être lue.
    p = scalefx(26_305.0)
    derive = next(w for w in p.warnings if w.code == "drift")
    assert derive.level == "info"
    assert p.zone == "green"


def test_une_derive_qui_compte_vraiment_se_voit_dans_la_zone():
    # Si la dérive a une conséquence, elle est déjà dans le ratio. C'est la
    # zone qui réclame un humain, pas l'avertissement.
    p = scalefx(26_320.0)
    assert all(w.level == "info" for w in p.warnings)
    assert p.zone == "amber"

# --- Espérance -----------------------------------------------------------------

# Distribution relevée sur les 132 trades XAUUSD du journal, novembre 2026.
SCALE_FX = plan.Outcomes(
    reached={3: 0.492, 2: 0.121, 1: 0.136, 0: 0.023},
    stop=0.227,
)


def gabarit_or():
    """Le gabarit Scale FX sur l'or : stop 80 pips, cibles 40 / 80 / 120."""
    return plan.build(
        spec=GOLD, direction="LONG", stop_loss=4102.00,
        targets=[4114.00, 4118.00, 4122.00],
        equity=100_000.0, risk_pct=0.01, market_price=4110.00,
    )


def test_lesperance_reproduit_le_calcul_fait_sur_les_donnees():
    # Calcul indépendant sur les 132 trades : +0,3485 R par trade. L'écart
    # vient de l'arrondi des jambes, 0,41 / 0,41 / 0,42 au lieu de tiers
    # exacts.
    p = gabarit_or()
    assert [leg.r_multiple for leg in p.legs] == [0.5, 1.0, 1.5]
    assert abs(SCALE_FX.expectancy(p.legs, p.total_volume) - 0.3485) < 0.005


def test_un_stop_coute_toujours_un_r_quelle_que_soit_sa_distance():
    # Le volume est dimensionné pour risquer le montant voulu, donc la perte
    # est constante en argent. Un gabarit plus large ne perd pas davantage.
    serre = plan.build(spec=GOLD, direction="LONG", stop_loss=4108.00,
                       targets=[4112.00, 4114.00, 4116.00],
                       equity=100_000.0, risk_pct=0.01, market_price=4110.00)
    perdant = plan.Outcomes(reached={3: 0, 2: 0, 1: 0, 0: 0}, stop=1.0)
    assert perdant.expectancy(serre.legs, serre.total_volume) == -1.0
    assert perdant.expectancy(gabarit_or().legs, gabarit_or().total_volume) == -1.0


def test_lesperance_tombe_avec_le_retard():
    # C'est ce que mesure la comparaison « entrer ou attendre ».
    a_lheure = gabarit_or()
    tardif = plan.build(spec=GOLD, direction="LONG", stop_loss=4102.00,
                        targets=[4114.00, 4118.00, 4122.00],
                        equity=100_000.0, risk_pct=0.01, market_price=4111.00)
    assert (SCALE_FX.expectancy(tardif.legs, tardif.total_volume)
            < SCALE_FX.expectancy(a_lheure.legs, a_lheure.total_volume))


def test_une_distribution_vide_ne_rend_pas_zero_par_accident():
    # Un volume nul doit rendre 0 sans diviser par zéro.
    assert SCALE_FX.expectancy([], 0.0) == 0.0


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
