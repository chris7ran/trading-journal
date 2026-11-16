"""Verser dans le journal l'historique déjà vécu par un terminal.

    py -3.12 backfill.py --account goat-2step --from 2026-10-19
    py -3.12 backfill.py --account goat-2step --account goat-paylater --from 2026-10-19 --dry-run

Ce script ne réutilise pas `journal.to_trades`, et c'est délibéré. Le watcher
ne remonte que ce que le pont a posé lui-même, reconnaissable à son numéro
magique : sans ce filtre, le journal se remplirait de tout ce qu'on ouvre à la
main dans le terminal, et la mesure d'un système automatique mélangerait ses
trades avec les autres.

Un backfill veut précisément le contraire. Il sert à récupérer ce qui existait
*avant* le pont — donc des trades sans magic, par construction. Ils partent
avec `plan_id` vide, ce qui les laisse distinguables plus tard.

L'opération est **idempotente** : la clé d'un trade est `login:ticket`, et le
backend remplace sans dupliquer. La relancer deux fois ne crée rien en double,
et ne touche jamais aux champs que tu as renseignés toi-même dans l'app.

Prérequis : le compte doit exister côté journal avec son `mt5_login`, sinon
l'ingestion n'a pas de destination et répond 404.

Environnement : `MCP_MT5_URL`, `MCP_MT5_TOKEN`, `JOURNAL_URL`,
`JOURNAL_INGEST_TOKEN` — les mêmes que le pont.
"""

from __future__ import annotations

import argparse
import os
import sys
from datetime import datetime, timedelta, timezone

import journal
import mcpclient

#: Taille d'un envoi. Un historique de plusieurs centaines de trades passerait
#: sans doute d'un coup, mais un lot refusé en bloc pour une seule ligne
#: fautive coûte cher à diagnostiquer.
TAILLE_LOT = 100

#: Fenêtre interrogée en une fois. `get_history` regroupe les deals par
#: position côté terminal, et une requête de plusieurs mois peut dépasser le
#: délai d'attente du pont. Un mois à la fois reste confortable.
FENETRE_JOURS = 30


def trades_du_terminal(client: mcpclient.Client, alias: str,
                       debut: str, fin: str) -> list[dict]:
    """Tous les trades clôturés d'une fenêtre, magic ou non."""
    reponse = client.call(
        "get_history",
        {"account": alias, "date_from": debut, "date_to": fin},
    )

    lot = []
    for trade in reponse.get("trades", []):
        if not trade.get("closed"):
            continue
        magic = trade.get("magic") or 0
        lot.append({
            "ticket": str(trade["ticket"]),
            # Le pont a déjà traduit le nom du broker en nom de journal :
            # `XAUUSD.x` devient `XAUUSD`, et c'est sous ce nom que l'app
            # regroupe les statistiques d'un instrument entre deux brokers.
            "symbol": trade.get("journal_symbol") or trade["symbol"],
            "direction": trade["direction"],
            "open_time": trade.get("open_time"),
            "close_time": trade.get("close_time"),
            "open_price": trade.get("open_price"),
            "close_price": trade.get("close_price"),
            "lot_size": trade.get("volume"),
            "pnl": trade.get("profit"),
            "commission": trade.get("commission"),
            "swap": trade.get("swap"),
            # Vide pour un trade manuel : il n'appartient à aucun plan, et
            # inventer un identifiant le ferait passer pour automatique.
            "plan_id": str(magic) if magic else None,
        })
    return lot


def fenetres(debut: str, fin: str) -> list[tuple[str, str]]:
    """Découper la période en tranches que le terminal digère."""
    d = datetime.strptime(debut, "%Y-%m-%d").replace(tzinfo=timezone.utc)
    f = datetime.strptime(fin, "%Y-%m-%d").replace(tzinfo=timezone.utc)
    if f < d:
        raise SystemExit("la date de fin précède la date de début")

    tranches = []
    while d <= f:
        suivant = min(d + timedelta(days=FENETRE_JOURS), f)
        tranches.append((d.strftime("%Y-%m-%d"), suivant.strftime("%Y-%m-%d")))
        if suivant == f:
            break
        d = suivant + timedelta(days=1)
    return tranches


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--account", action="append", required=True,
                        help="alias du compte ; répétable")
    parser.add_argument("--from", dest="debut", required=True,
                        help="première journée incluse, YYYY-MM-DD")
    parser.add_argument("--to", dest="fin",
                        default=datetime.now(timezone.utc).strftime("%Y-%m-%d"),
                        help="dernière journée incluse, aujourd'hui par défaut")
    parser.add_argument("--dry-run", action="store_true",
                        help="compter et afficher, sans rien envoyer")
    args = parser.parse_args()

    url_journal = os.environ.get("JOURNAL_URL", "").strip()
    jeton_journal = os.environ.get("JOURNAL_INGEST_TOKEN", "").strip()
    if not args.dry_run and not (url_journal and jeton_journal):
        print("JOURNAL_URL et JOURNAL_INGEST_TOKEN sont requis", file=sys.stderr)
        return 2

    client = mcpclient.Client(
        os.environ.get("MCP_MT5_URL", "http://127.0.0.1:8765/mcp"),
        os.environ.get("MCP_MT5_TOKEN", ""),
    )
    try:
        client.connect()
        comptes = {c["alias"]: c for c in client.call("list_accounts")["accounts"]}
    except mcpclient.McpError as exc:
        print(f"pont injoignable : {exc}", file=sys.stderr)
        return 2

    destination = journal.Journal(url_journal, jeton_journal) if not args.dry_run else None

    for alias in args.account:
        if alias not in comptes:
            print(f"compte inconnu du pont : {alias} "
                  f"(connus : {', '.join(comptes)})", file=sys.stderr)
            return 2

        login = comptes[alias]["login"]
        tout = []
        for debut, fin in fenetres(args.debut, args.fin):
            try:
                tranche = trades_du_terminal(client, alias, debut, fin)
            except mcpclient.McpError as exc:
                print(f"[{alias}] {debut} → {fin} : {exc}", file=sys.stderr)
                return 1
            print(f"[{alias}] {debut} → {fin} : {len(tranche)} trade(s)")
            tout.extend(tranche)

        # Une même position peut apparaître dans deux fenêtres si elle est
        # ouverte de part et d'autre d'une frontière. Le backend dédoublonne
        # de toute façon, mais autant ne pas l'envoyer deux fois.
        par_ticket = {t["ticket"]: t for t in tout}
        lot_complet = list(par_ticket.values())

        manuels = sum(1 for t in lot_complet if not t["plan_id"])
        print(f"[{alias}] {len(lot_complet)} trade(s) au total, "
              f"dont {manuels} sans plan")

        if args.dry_run:
            for t in lot_complet[:5]:
                print(f"    {t['close_time']}  {t['symbol']:8} {t['direction']:5} "
                      f"{t['lot_size']}  {t['pnl']:+.2f}")
            if len(lot_complet) > 5:
                print(f"    … et {len(lot_complet) - 5} autres")
            continue

        recus = crees = maj = 0
        for i in range(0, len(lot_complet), TAILLE_LOT):
            morceau = lot_complet[i:i + TAILLE_LOT]
            try:
                r = destination.push(login, morceau)
            except journal.JournalError as exc:
                print(f"[{alias}] envoi refusé : {exc}", file=sys.stderr)
                print("Le compte existe-t-il dans l'app avec ce login MT5 ?",
                      file=sys.stderr)
                return 1
            recus += r.get("received", 0)
            crees += r.get("created", 0)
            maj += r.get("updated", 0)

        print(f"[{alias}] journal : {recus} reçu(s), {crees} créé(s), {maj} mis à jour")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
