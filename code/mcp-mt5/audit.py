"""Trace append-only de tout ce qui est envoyé au broker.

Une règle gouverne ce module : **pas de trace, pas d'ordre.** Si l'écriture
échoue, l'envoi n'a pas lieu. C'est ce qui sépare un journal d'audit d'un
fichier de logs — un log qu'on perd fait un trou dans l'histoire, un audit
qu'on perd fait un ordre dont personne ne peut dire d'où il vient.

De là découle le reste de la conception.

**Deux écritures par envoi.** L'intention est écrite *avant* d'appeler le
broker, le résultat *après*. Une intention sans résultat est le signal le plus
précieux du fichier : elle dit qu'un ordre est parti sans qu'on sache ce qu'il
est devenu — panne de réseau, terminal tué, machine coupée. Écrire seulement
après laisserait ce cas parfaitement invisible.

**`fsync` à chaque ligne.** Un tampon système qui disparaît dans une coupure
emporte précisément les dernières lignes, c'est-à-dire celles qui comptent.

**Un fichier par jour, jamais réouvert en écriture ailleurs qu'en fin.** Rien
dans ce module ne sait modifier ni supprimer une ligne déjà écrite.
"""

from __future__ import annotations

import json
import os
import socket
from datetime import datetime, timezone
from pathlib import Path


class AuditError(Exception):
    """L'écriture a échoué. L'appelant doit renoncer à envoyer."""


class Audit:
    """Journal d'un compte, sur disque."""

    def __init__(self, directory: Path, account: str) -> None:
        self.directory = Path(directory)
        self.account = account
        try:
            self.directory.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            raise AuditError(
                f"journal d'audit inaccessible à {self.directory} : {exc}"
            ) from exc
        # Écrire une ligne au démarrage vérifie le disque tant qu'aucun ordre
        # n'en dépend encore. Découvrir que F: est plein au moment d'envoyer
        # serait découvrir trop tard.
        self.write("bridge_started", {"host": socket.gethostname(), "pid": os.getpid()})

    @property
    def path(self) -> Path:
        day = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        return self.directory / f"{day}.jsonl"

    def write(self, event: str, payload: dict, plan_id: str | None = None) -> dict:
        """Ajouter une ligne, ou lever. Ne renvoie jamais sans avoir écrit."""
        record = {
            "at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
            "account": self.account,
            "event": event,
            "plan_id": plan_id,
            "payload": payload,
        }
        line = json.dumps(record, ensure_ascii=False, default=str)

        try:
            with self.path.open("a", encoding="utf-8") as handle:
                handle.write(line + "\n")
                handle.flush()
                os.fsync(handle.fileno())
        except OSError as exc:
            raise AuditError(
                f"écriture du journal d'audit impossible ({self.path}) : {exc}. "
                f"Aucun ordre ne sera envoyé."
            ) from exc

        return record

    def read_day(self, day: str | None = None) -> list[dict]:
        """Relire une journée. Lecture seule, pour l'inspection et les tests."""
        target = self.directory / f"{day or datetime.now(timezone.utc):%Y-%m-%d}.jsonl"
        if isinstance(day, str):
            target = self.directory / f"{day}.jsonl"
        if not target.is_file():
            return []
        records = []
        for line in target.read_text(encoding="utf-8").splitlines():
            if line.strip():
                try:
                    records.append(json.loads(line))
                except json.JSONDecodeError:
                    # Une ligne tronquée par une coupure n'invalide pas le
                    # reste du fichier, et la perdre en silence serait pire
                    # que la signaler.
                    records.append({"event": "ligne_illisible", "raw": line})
        return records

    def orphans(self, day: str | None = None) -> list[dict]:
        """Intentions sans résultat : les ordres dont l'issue est inconnue.

        C'est la question qu'on pose à ce fichier après un incident, et la
        seule qui justifie d'avoir écrit deux fois par envoi.
        """
        records = self.read_day(day)
        resolved = {
            r.get("plan_id")
            for r in records
            if r.get("event") in ("order_result", "order_failed")
        }
        return [
            r
            for r in records
            if r.get("event") == "order_intent" and r.get("plan_id") not in resolved
        ]
