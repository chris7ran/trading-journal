"""Tests de la mesure d'horloge et des premiers relevés, sans terminal.

    py -3.12 test_clock.py

Trois bugs sont déjà passés par ici : un symbole périmé qui tire la mesure à
UTC+1,5, un rapport qui annonçait « aucun symbole ne cote » en affichant onze
symboles cotants, et un premier chiffrage refusé pour « marché fermé » sur un
symbole qui venait d'être abonné.

Ils ont tous la même racine : un terminal qui vient de démarrer, ou un symbole
qu'on vient d'ajouter, répond du vide avant de répondre juste. Les trois se
reproduisent ici.
"""

from __future__ import annotations

import time
from types import SimpleNamespace

import worker

# Pas de vraie attente : le terminal simulé n'a pas d'historique à télécharger.
worker.PAUSE_HISTORIQUE_SECONDS = 0


class FauxMT5:
    """Rend une bougie M1 par symbole, à l'ancienneté qu'on lui dicte.

    `ages` donne, par symbole, l'ancienneté de sa dernière bougie en secondes.
    `None` veut dire « pas de données », le cas du terminal qui n'a pas encore
    téléchargé son historique.
    """

    TIMEFRAME_M1 = 1

    def __init__(self, decalage: int, ages: dict[str, int | None]) -> None:
        self.decalage = decalage
        self.ages = ages
        self.appels = 0

    def symbols_get(self):
        return [SimpleNamespace(name=n, visible=True) for n in self.ages]

    def copy_rates_from_pos(self, symbol, timeframe, start, count):
        self.appels += 1
        age = self.ages.get(symbol)
        if age is None:
            return []
        # L'horodatage MT5 est l'heure serveur : UTC + décalage, moins l'âge.
        return [{"time": int(time.time()) + self.decalage - age}]


def etat(mt5):
    return SimpleNamespace(mt5=mt5, offset=None, offset_measured_at=None)


# --- La mesure elle-même --------------------------------------------------------


def test_un_symbole_frais_donne_le_decalage():
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": 10})
    offset, echantillon = worker._mesurer_offset(etat(mt5))
    assert offset == 7200
    assert len(echantillon) == 1


def test_le_symbole_le_moins_perime_gagne():
    # Le 09/11 : le GER40, fermé, rendait une bougie de la veille et tirait la
    # mesure à UTC+1,5. Prendre le maximum, c'est prendre le plus frais.
    mt5 = FauxMT5(decalage=7200, ages={"GER40.x": 5400, "XAUUSD.x": 20})
    offset, _ = worker._mesurer_offset(etat(mt5))
    assert offset == 7200


def test_tous_perimes_mais_dans_la_bande_sarrondissent_quand_meme():
    # Aucun symbole frais : la mesure sort fausse, et c'est assumé — la grille
    # horaire absorbe les petits retards, pas une bougie vieille d'une heure.
    mt5 = FauxMT5(decalage=7200, ages={"GER40.x": 3600})
    offset, _ = worker._mesurer_offset(etat(mt5))
    assert offset == 3600


def test_une_bougie_absurde_est_ecartee():
    # Au-delà d'une demi-journée, ce n'est plus un décalage de serveur : c'est
    # la clôture de vendredi relue un dimanche.
    mt5 = FauxMT5(decalage=7200, ages={"GER40.x": 20 * 3600})
    offset, echantillon = worker._mesurer_offset(etat(mt5))
    assert offset is None
    assert len(echantillon) == 1  # vue, mais pas retenue


def test_larrondi_tombe_sur_lheure_pleine():
    # 7 174 s mesurées chez GoatFunded le 16/11 : c'est UTC+2, à 26 secondes
    # de latence près.
    mt5 = FauxMT5(decalage=7174, ages={"XAUUSD.x": 0})
    offset, _ = worker._mesurer_offset(etat(mt5))
    assert offset == 7200


def test_aucune_donnee_ne_produit_aucune_mesure():
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": None, "GER40.x": None})
    offset, echantillon = worker._mesurer_offset(etat(mt5))
    assert offset is None
    assert echantillon == []


# --- Le démarrage à froid -------------------------------------------------------


class FauxMT5Froid(FauxMT5):
    """Vide au premier passage, peuplé ensuite.

    C'est le comportement réel du terminal : l'interrogation déclenche le
    téléchargement de l'historique, donc le premier appel rend du vide et le
    suivant les données.
    """

    def __init__(self, decalage, ages):
        super().__init__(decalage, ages)
        self.passages = 0
        self._vrais_ages = dict(ages)
        self.ages = {k: None for k in ages}

    def symbols_get(self):
        # Un passage = un tour complet sur les symboles.
        if self.passages >= 1:
            self.ages = dict(self._vrais_ages)
        self.passages += 1
        return super().symbols_get()


def test_un_terminal_froid_est_reinterroge_une_fois():
    mt5 = FauxMT5Froid(decalage=7200, ages={"XAUUSD.x": 15})
    offset, echantillon = worker._mesurer_offset(etat(mt5))
    assert offset == 7200, "la seconde tentative doit aboutir"
    assert echantillon


# --- La cohérence du rapport ----------------------------------------------------


def test_le_rapport_ne_se_contredit_pas():
    # Le bug du 16/11 : onze symboles affichés avec leur décalage, sous la
    # mention « aucun symbole ne cote ». Un rapport incoherent est pire qu'un
    # rapport vide, parce qu'on ne sait plus lequel des deux croire.
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": 10, "EURUSD.x": 20})
    rapport = worker._clock(etat(mt5))

    assert rapport["measured"] is True
    assert rapport["server_utc_offset_seconds"] == 7200
    assert rapport["samples"]
    assert "aucune bougie lisible" not in rapport["note"]


def test_un_rapport_sans_mesure_le_dit_et_nexhibe_rien():
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": None})
    rapport = worker._clock(etat(mt5))

    assert rapport["measured"] is False
    assert rapport["samples"] == []
    assert "aucune bougie lisible" in rapport["note"]


def test_lanciennete_se_lit_par_rapport_au_decalage_retenu():
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": 0, "GER40.x": 1800})
    rapport = worker._clock(etat(mt5))

    par_symbole = {s["symbol"]: s for s in rapport["samples"]}
    assert par_symbole["XAUUSD.x"]["stale_seconds"] == 0
    assert par_symbole["GER40.x"]["stale_seconds"] == 1800


def test_une_mesure_en_cache_survit_a_la_fermeture_du_marche():
    # Marché fermé : plus aucune bougie fraîche, mais le décalage mesuré hier
    # reste vrai. Le perdre obligerait à refuser tout ordre en attente.
    mt5 = FauxMT5(decalage=7200, ages={"XAUUSD.x": None})
    e = etat(mt5)
    e.offset = 7200
    e.offset_measured_at = time.time() - 3600

    rapport = worker._clock(e)
    assert rapport["server_utc_offset_seconds"] == 7200
    assert rapport["measured"] is True


# --- Le premier tick d'un symbole fraîchement abonné ----------------------------

worker.PAUSE_COTATION_SECONDS = 0  # pas de serveur à attendre non plus


class FauxTicks:
    """Rend un tick vide au premier appel, puis un vrai prix."""

    def __init__(self, bid=3848.0, ask=3848.5, vides=1):
        self.bid, self.ask = bid, ask
        self.restants = vides
        self.appels = 0

    def symbol_select(self, symbol, visible):
        return True

    def symbols_get(self):
        # `_quote` date son tick, ce qui passe par la mesure d'horloge. Sans
        # candidat, elle rend None et l'horodatage reste en heure serveur —
        # ce que ce test ne juge pas.
        return []

    def symbol_info_tick(self, symbol):
        self.appels += 1
        if self.restants > 0:
            self.restants -= 1
            return SimpleNamespace(bid=0.0, ask=0.0, last=0.0, time=0)
        return SimpleNamespace(bid=self.bid, ask=self.ask, last=0.0,
                               time=int(time.time()) + 7200)

    def symbol_info(self, symbol):
        return SimpleNamespace(point=0.01)

    def last_error(self):
        return (0, "")


def test_un_symbole_juste_abonne_est_reinterroge():
    # Le 16/11 sur goat-paylater : `symbol_select` reussit, mais le premier
    # tick arrive vide et le plan etait refuse pour « marche ferme ».
    mt5 = FauxTicks(vides=1)
    cote = worker._quote(etat(mt5), "XAUUSD.x")
    assert cote["quoting"] is True
    assert cote["bid"] == 3848.0
    assert mt5.appels == 2


def test_un_marche_vraiment_ferme_reste_signale_comme_tel():
    # La seconde chance ne doit pas transformer une fermeture en cotation :
    # deux tentatives vides restent deux tentatives vides.
    mt5 = FauxTicks(vides=99)
    cote = worker._quote(etat(mt5), "XAUUSD.x")
    assert cote["quoting"] is False
    assert cote["bid"] == 0.0


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
