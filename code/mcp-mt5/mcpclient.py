"""Client MCP minimal, partagé par les outils en ligne de commande et le bot.

Extrait de `probe.py`, qui en avait besoin le premier. Deux clients pour le
même serveur auraient fini par diverger — et c'est celui qu'on regarde le
moins qui aurait eu tort.

Bibliothèque standard uniquement : ce client tourne à côté du serveur, sur une
machine où toute dépendance de plus est une dépendance à maintenir.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request

PROTOCOL_VERSION = "2025-06-18"


class McpError(Exception):
    """Le serveur a refusé, ou n'a pas répondu."""


class Client:
    """Une session MCP sur un serveur en Streamable HTTP."""

    def __init__(self, url: str, token: str = "", *, timeout: int = 180) -> None:
        self.url = url
        self.token = token
        self.timeout = timeout
        self.session: str | None = None
        self._id = 0

    # --- Protocole ----------------------------------------------------------

    def connect(self) -> None:
        """Poignée de main. À appeler une fois avant tout appel d'outil."""
        self._post({
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "trading-journal-client", "version": "1"},
            },
        })
        # Notification sans identifiant : certains serveurs attendent de
        # l'avoir reçue avant d'accepter le moindre appel.
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def tools(self) -> list[dict]:
        reponse = self._post(
            {"jsonrpc": "2.0", "id": self._next_id(), "method": "tools/list"}
        )
        return (reponse or {}).get("result", {}).get("tools", [])

    def call(self, name: str, arguments: dict | None = None) -> dict:
        """Appeler un outil et rendre sa valeur structurée.

        Un échec d'outil arrive en `isError` dans un résultat réussi, pas en
        erreur de protocole : c'est la convention MCP, et elle est la bonne —
        le message est fait pour être lu et corrigé, pas pour couper la
        conversation. Il est donc remonté comme une exception portant le texte
        du serveur.
        """
        reponse = self._post({
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}},
        }) or {}

        if "error" in reponse:
            raise McpError(reponse["error"].get("message", "erreur inconnue"))

        resultat = reponse.get("result", {})
        contenu = resultat.get("content") or []
        texte = contenu[0].get("text", "") if contenu else ""

        if resultat.get("isError"):
            raise McpError(texte or "l'outil a échoué sans message")

        valeur = resultat.get("structuredContent")
        if valeur is None:
            try:
                valeur = json.loads(texte)
            except json.JSONDecodeError:
                valeur = {"text": texte}
        return valeur

    # --- Transport ----------------------------------------------------------

    def _next_id(self) -> int:
        self._id += 1
        return self._id

    def _post(self, payload: dict, _reessai: bool = True) -> dict | None:
        """Envoyer, et refaire la poignée de main si la session a expiré.

        Le serveur ferme une session restée inactive, et répond alors 404 au
        prochain appel. Sans reconnexion, un client qui vit des heures entre
        deux usages — un bot qui attend un signal — reste cassé jusqu'à ce
        qu'on le relance, et il échoue au moment précis où on avait besoin de
        lui.

        La reprise est silencieuse et n'a lieu qu'une fois : une session
        refusée deux fois de suite n'est plus un problème de session.
        """
        requete = urllib.request.Request(
            self.url, data=json.dumps(payload).encode(), method="POST"
        )
        requete.add_header("Content-Type", "application/json")
        # Streamable HTTP laisse le serveur répondre en JSON simple ou en flux
        # d'un seul événement, et il choisit d'après cet en-tête : n'en
        # proposer qu'un seul fait refuser certains serveurs.
        requete.add_header("Accept", "application/json, text/event-stream")
        if self.token:
            requete.add_header("Authorization", f"Bearer {self.token}")
        if self.session:
            requete.add_header("Mcp-Session-Id", self.session)

        try:
            with urllib.request.urlopen(requete, timeout=self.timeout) as reponse:
                session = reponse.headers.get("Mcp-Session-Id")
                if session:
                    self.session = session
                return _decode(reponse.read().decode("utf-8", "replace"))
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")[:300]
            session_expiree = (
                exc.code in (400, 404)
                and self.session is not None
                and payload.get("method") != "initialize"
            )
            if session_expiree and _reessai:
                self.session = None
                self.connect()
                return self._post(payload, _reessai=False)
            raise McpError(f"HTTP {exc.code} : {detail or exc.reason}") from exc
        except urllib.error.URLError as exc:
            raise McpError(f"serveur injoignable sur {self.url} : {exc.reason}") from exc


def _decode(body: str) -> dict | None:
    """Lire une réponse en JSON simple ou en événement SSE unique."""
    body = body.strip()
    if not body:
        return None
    if body.startswith("{"):
        return json.loads(body)
    for ligne in body.splitlines():
        if ligne.startswith("data:"):
            return json.loads(ligne[5:].strip())
    return None
