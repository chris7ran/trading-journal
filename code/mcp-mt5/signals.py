"""Lecture des signaux transférés depuis Telegram.

Trois émetteurs, trois formats, et aucun qui ressemble aux autres : libellés
anglais ou français, entrée en prix unique ou en fourchette, stop avant ou
après les cibles, deux ou trois objectifs, et des emojis partout. L'analyse
est donc volontairement **tolérante sur la forme et stricte sur le fond** —
elle cherche des libellés et des nombres, pas une grammaire.

Ce module ne décide rien. Il extrait ce qui est écrit et le rend tel quel,
fourchette comprise avec ses deux bords dans l'ordre d'écriture. Choisir le
bord de référence, le découpage en jambes ou le risque est l'affaire de
l'appelant — et du trader.

**Ce qui n'est pas lu est signalé.** Un champ manquant produit une erreur
nommée, jamais une valeur par défaut : un signal à moitié compris qui
produirait quand même un plan serait la pire des sorties.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

LONG = "LONG"
SHORT = "SHORT"

#: Noms d'instruments tels qu'ils apparaissent dans les messages, et leur
#: équivalent dans le journal. Les alias les plus longs sont essayés d'abord,
#: pour que « XAU/USD » ne soit pas coupé par « XAU ».
ALIASES = {
    "XAUUSD": ["XAU/USD", "XAUUSD", "XAU USD", "GOLD", "OR "],
    "NAS100": ["NASDAQ", "NAS100", "US100", "USTEC", "NDX"],
    "US30": ["US30", "DOW JONES", "DOW", "DJ30"],
    "GER40": ["GER40", "DAX40", "DAX", "DE40", "GER30"],
    "US500": ["SP500", "S&P500", "S&P 500", "US500"],
    "EURUSD": ["EUR/USD", "EURUSD", "EUR USD"],
    "GBPUSD": ["GBP/USD", "GBPUSD", "GBP USD"],
    "USDJPY": ["USD/JPY", "USDJPY", "USD JPY"],
    "BTCUSD": ["BTC/USD", "BTCUSD", "BITCOIN", "BTC "],
    "ETHUSD": ["ETH/USD", "ETHUSD", "ETHEREUM"],
    "USOIL": ["USOIL", "WTI", "CRUDE"],
}

#: Mots qui annoncent un achat ou une vente, dans les trois langues croisées.
ACHAT = ("BUY", "LONG", "ACHAT")
VENTE = ("SELL", "SHORT", "VENTE")

#: Libellés d'entrée. `PE` pour « prix d'entrée » chez goldmind.
ETIQUETTES_ENTREE = ("ENTRY", "ENTREE", "ENTRÉE", "PE", "PRIX D'ENTREE", "ENTRADA")
ETIQUETTES_STOP = ("SL", "STOP LOSS", "STOPLOSS", "STOP")

#: Un nombre, décimale au point ou à la virgule.
#:
#: Deux branches, et l'ordre compte. La première exige un **vrai** séparateur
#: de milliers — « 26 850 ». Le rendre facultatif ferait correspondre
#: « \\d{1,3} » aux trois premiers chiffres de « 4053 » et laisserait le
#: quatrième de côté : le prix serait lu 405, sans que rien ne signale
#: l'amputation.
NOMBRE = r"\d{1,3}(?:[  ]\d{3})+(?:[.,]\d+)?|\d+(?:[.,]\d+)?"

#: Consignes de gestion qu'on relève sans les appliquer : elles décrivent le
#: découpage voulu par l'émetteur, et c'est au trader de dire s'il le suit.
CONSIGNES = (
    # « Half + BE », « Half Close + Break Even (BE) », « 50% + BE »… Les mots
    # qui s'intercalent varient d'un émetteur à l'autre ; ce qui ne varie pas,
    # c'est la paire moitié / point mort.
    (
        r"(?:half|moiti[ée]|50\s*%)[^\n]{0,20}?(?:\bbe\b|break\s*even)",
        "moitié sortie à TP1 puis point mort",
    ),
    (r"\bbe\s+obligatoire", "point mort obligatoire après TP1"),
    (r"risque?\s*(?:max\w*)?\s*:?\s*(\d+(?:[.,]\d+)?)\s*%", "risque conseillé : {} %"),
    (r"risque\s+(\d+(?:[.,]\d+)?)\s*r\b", "risque conseillé : {} R"),
)


class SignalError(Exception):
    """Le message n'est pas un signal exploitable. Le message le dit."""


@dataclass(frozen=True)
class Signal:
    symbol: str
    raw_symbol: str
    direction: str
    stop_loss: float
    targets: list[float]
    #: Prix unique, quand il n'y a pas de fourchette.
    entry: float | None = None
    #: Les deux bords **dans l'ordre d'écriture**. Scale FX écrit du plus haut
    #: au plus bas pour un achat, et c'est le premier nombre qu'ils
    #: enregistrent dans leur propre journal — donc l'ordre porte du sens et
    #: ne doit pas être trié ici.
    entry_range: tuple[float, float] | None = None
    notes: list[str] = field(default_factory=list)

    @property
    def entry_first(self) -> float:
        """Le premier prix écrit : la convention de Scale FX."""
        if self.entry_range:
            return self.entry_range[0]
        return self.entry  # type: ignore[return-value]

    @property
    def entry_best(self) -> float:
        """Le bord le plus favorable : bas pour un achat, haut pour une vente."""
        if not self.entry_range:
            return self.entry  # type: ignore[return-value]
        bas, haut = sorted(self.entry_range)
        return bas if self.direction == LONG else haut


def parse(message: str) -> Signal:
    """Lire un signal transféré. Lève `SignalError` si quelque chose manque."""
    if not message or not message.strip():
        raise SignalError("message vide")

    texte = message.replace(" ", " ")
    majuscules = texte.upper()

    direction = _direction(majuscules)
    brut, symbole = _symbole(majuscules)
    entree, fourchette = _entree(texte)
    stop = _stop(texte)
    cibles = _cibles(texte)

    if not cibles:
        raise SignalError("aucune cible trouvée : cherchez TP1, TP 1 ou Target")

    return Signal(
        symbol=symbole,
        raw_symbol=brut,
        direction=direction,
        stop_loss=stop,
        targets=cibles,
        entry=entree,
        entry_range=fourchette,
        notes=_consignes(texte),
    )


# --- Champs -------------------------------------------------------------------


def _direction(majuscules: str) -> str:
    """Le sens, cherché comme mot entier.

    Entier, sinon « SELL » se trouverait dans un nom de canal et « OR » dans
    n'importe quelle phrase française.
    """
    for mot in ACHAT:
        if re.search(rf"\b{mot}\b", majuscules):
            return LONG
    for mot in VENTE:
        if re.search(rf"\b{mot}\b", majuscules):
            return SHORT
    raise SignalError("sens introuvable : ni achat ni vente reconnu")


def _symbole(majuscules: str) -> tuple[str, str]:
    """L'instrument, le plus long alias d'abord."""
    candidats = [
        (alias, journal)
        for journal, alias_list in ALIASES.items()
        for alias in alias_list
    ]
    for alias, journal in sorted(candidats, key=lambda c: -len(c[0])):
        if alias in majuscules:
            return alias.strip(), journal
    raise SignalError("instrument introuvable dans le message")


def _entree(texte: str) -> tuple[float | None, tuple[float, float] | None]:
    """Le prix d'entrée, unique ou en fourchette.

    La fourchette exige un séparateur entouré d'espaces. Sans cette exigence,
    « SL : 26960 -110 » — où le tiret colle au nombre et désigne une distance
    — serait lu comme une fourchette de 26 960 à 110.
    """
    ligne = _ligne_avec(texte, ETIQUETTES_ENTREE)
    if ligne is None:
        raise SignalError("prix d'entrée introuvable : cherchez Entry, Entrée ou PE")

    fourchette = re.search(rf"({NOMBRE})\s+[-–—àa/]\s+({NOMBRE})", ligne)
    if fourchette:
        return None, (_nombre(fourchette.group(1)), _nombre(fourchette.group(2)))

    nombres = re.findall(NOMBRE, ligne)
    if not nombres:
        raise SignalError("prix d'entrée illisible")
    return _nombre(nombres[0]), None


def _stop(texte: str) -> float:
    """Le stop : **le premier nombre** de sa ligne, et rien d'autre.

    goldmind écrit « SL : 26960 -110 », où le second nombre est la distance en
    points. Prendre autre chose que le premier donnerait un stop à 110.
    """
    ligne = _ligne_avec(texte, ETIQUETTES_STOP)
    if ligne is None:
        raise SignalError("stop introuvable : cherchez SL ou Stop")
    nombres = re.findall(NOMBRE, ligne)
    if not nombres:
        raise SignalError("stop illisible")
    return _nombre(nombres[0])


def _cibles(texte: str) -> list[float]:
    """Les objectifs, numérotés ou listés sur une seule ligne.

    Deux écritures coexistent chez les émetteurs. La numérotée, « TP1 : 4126 »
    sur une ligne chacune. Et la groupée, « TP 74170 / 73500 / 73000 », où
    aucun numéro n'apparaît et où l'ordre d'écriture fait foi.

    La forme numérotée est cherchée en premier et exige une **frontière de
    mot** après le numéro. Sans cette exigence, « TP 74170 » se lirait
    « TP 7 » suivi du prix 4170 : une cible inventée, plausible, et fausse de
    deux ordres de grandeur. C'est le genre d'erreur qui ne se voit pas sur un
    écran de téléphone.
    """
    numerotees: dict[int, float] = {}
    for ligne in texte.splitlines():
        marque = re.search(r"\bTP\s*(\d)\b|\bTARGET\s*(\d)\b", ligne, re.IGNORECASE)
        if not marque:
            continue
        numero = int(marque.group(1) or marque.group(2))
        nombres = re.findall(NOMBRE, ligne[marque.end():])
        if nombres:
            numerotees[numero] = _nombre(nombres[0])
    if numerotees:
        # Triées sur le numéro et non sur la position : un émetteur peut les
        # écrire à l'envers, et c'est le numéro qui dit laquelle est la plus
        # proche.
        return [numerotees[n] for n in sorted(numerotees)]

    # Forme groupée. Cherchée seulement si la numérotée n'a rien donné, pour
    # qu'un « À TP1 : Half Close » ne vienne pas brouiller une liste propre.
    for ligne in texte.splitlines():
        if not re.search(r"\b(TP|TARGETS?|OBJECTIFS?)\b", ligne, re.IGNORECASE):
            continue
        nombres = re.findall(NOMBRE, ligne)
        if len(nombres) >= 2:
            return [_nombre(n) for n in nombres]
    return []


def _consignes(texte: str) -> list[str]:
    notes = []
    for motif, gabarit in CONSIGNES:
        trouve = re.search(motif, texte, re.IGNORECASE)
        if trouve:
            notes.append(
                gabarit.format(*trouve.groups()) if trouve.groups() else gabarit
            )
    return notes


# --- Outils --------------------------------------------------------------------


def _ligne_avec(texte: str, etiquettes: tuple[str, ...]) -> str | None:
    """La première ligne portant l'une des étiquettes, comme mot entier.

    Ligne par ligne plutôt que sur le message entier : « Entry » et « SL »
    vivent sur des lignes distinctes chez les trois émetteurs, et chercher
    globalement ramasserait les nombres du voisin.
    """
    for ligne in texte.splitlines():
        majuscule = ligne.upper()
        for etiquette in etiquettes:
            if re.search(rf"(^|[^A-Z]){re.escape(etiquette)}\s*[:：]?\s*", majuscule):
                return ligne
    return None


def _nombre(brut: str) -> float:
    return float(brut.replace(" ", "").replace(" ", "").replace(",", "."))
