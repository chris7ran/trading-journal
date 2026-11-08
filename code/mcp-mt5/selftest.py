"""Vérification du pont, sans MCP ni réseau.

Démarre chaque terminal configuré, lit le compte, l'horloge et le Market
Watch, puis relève les spécifications des symboles déclarés. À lancer après
toute modification de `accounts.toml`, et avant de démarrer le serveur : il
isole les problèmes de configuration et de terminal des problèmes de
protocole.

    py -3.12 selftest.py

Aucun ordre n'est envoyé — ce script n'appelle que des lectures.
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

import config
import server


async def check(terminal: server.Terminal) -> bool:
    alias = terminal.account.alias
    print(f"\n=== {alias} ({terminal.account.login} @ {terminal.account.server}) ===")

    try:
        account = await terminal.call("account")
    except Exception as exc:  # noqa: BLE001 - this script reports, it does not raise
        print(f"  ÉCHEC : {exc}")
        return False

    print(f"  devise        : {account['currency']}")
    print(f"  solde         : {account['balance']}")
    print(f"  équité        : {account['equity']}")
    print(f"  levier        : 1:{account['leverage']}")
    print(f"  mode marge    : {account['margin_mode']}")
    print(f"  algo trading  : {account['algo_trading_enabled']}")

    if account["margin_mode"] != "hedging":
        print("  ATTENTION : compte en netting, la gestion par jambes ne s'applique pas.")

    clock = await terminal.call("clock")
    if clock["measured"]:
        print(f"  horloge       : serveur à UTC{clock['server_utc_offset_hours']:+g} h")
    else:
        print(f"  horloge       : non mesurable — {clock['note']}")

    watch = await terminal.call("market_watch")
    print(f"  market watch  : {len(watch)} symboles")

    ok = True
    for journal_name in terminal.account.symbols:
        broker_name = terminal.account.to_broker(journal_name)
        try:
            spec = await terminal.call("symbol", symbol=broker_name)
        except Exception as exc:  # noqa: BLE001
            print(f"  {journal_name:<8} -> {broker_name:<14} ÉCHEC : {exc}")
            ok = False
            continue
        print(
            f"  {journal_name:<8} -> {spec['name']:<14} "
            f"pas={spec['volume_step']} min={spec['volume_min']} "
            f"3 jambes>={spec['min_volume_for_three_legs']} "
            f"exécution={','.join(spec['filling_modes']) or 'aucune'} "
            f"trade={spec['trade_mode']}"
        )
        if spec["trade_mode"] != "full":
            print(f"    ATTENTION : {spec['name']} n'est pas en trading complet.")

    return ok


async def main(path: Path | None) -> int:
    try:
        accounts = config.load(path)
        bridge = server.Bridge(accounts)
    except config.ConfigError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    print(f"{len(bridge.aliases)} compte(s) activé(s) : {', '.join(bridge.aliases)}")
    if bridge.disabled:
        print(f"désactivés : {', '.join(bridge.disabled)}")

    results = [await check(bridge.get(alias)) for alias in bridge.aliases]
    bridge.stop_all()

    failures = results.count(False)
    print(f"\n{len(results) - failures}/{len(results)} compte(s) prêt(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    import multiprocessing as mp

    mp.freeze_support()
    argument = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    raise SystemExit(asyncio.run(main(argument)))
