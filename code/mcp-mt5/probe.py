"""Client MCP minimal, pour tester le pont sans assistant.

Quand une réponse déçoit, la première question est toujours la même : est-ce
le terminal, le pont, ou le client ? Ce script retire le dernier des
suspects. Il n'utilise que la bibliothèque standard.

    py -3.12 probe.py                                   # liste les outils
    py -3.12 probe.py list_accounts
    py -3.12 probe.py get_account account=xm-demo
    py -3.12 probe.py get_history account=xm-demo date_from=2026-11-01

L'URL et le jeton viennent de MCP_MT5_URL et MCP_MT5_TOKEN, ou des options
--url et --token.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

DEFAULT_URL = "http://127.0.0.1:8765/mcp"
PROTOCOL_VERSION = "2025-06-18"


class Client:
    def __init__(self, url: str, token: str) -> None:
        self.url = url
        self.token = token
        self.session: str | None = None

    def post(self, payload: dict) -> dict | None:
        request = urllib.request.Request(
            self.url, data=json.dumps(payload).encode(), method="POST"
        )
        request.add_header("Content-Type", "application/json")
        # Streamable HTTP lets the server answer either as plain JSON or as a
        # one-event SSE stream, and it picks based on this header. Offering
        # only one of the two makes some servers refuse outright.
        request.add_header("Accept", "application/json, text/event-stream")
        if self.token:
            request.add_header("Authorization", f"Bearer {self.token}")
        if self.session:
            request.add_header("Mcp-Session-Id", self.session)

        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                session = response.headers.get("Mcp-Session-Id")
                if session:
                    self.session = session
                body = response.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")
            raise SystemExit(f"HTTP {exc.code} : {detail or exc.reason}") from exc
        except urllib.error.URLError as exc:
            raise SystemExit(f"serveur injoignable sur {self.url} : {exc.reason}") from exc

        return _decode(body)

    def handshake(self) -> None:
        self.post({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "probe", "version": "1"},
            },
        })
        # A notification carries no id and expects no reply; skipping it leaves
        # some servers waiting and refusing every later call.
        self.post({"jsonrpc": "2.0", "method": "notifications/initialized"})


def _decode(body: str) -> dict | None:
    """Parse a reply that may be bare JSON or a single SSE event."""
    body = body.strip()
    if not body:
        return None
    if body.startswith("{"):
        return json.loads(body)
    for line in body.splitlines():
        if line.startswith("data:"):
            return json.loads(line[5:].strip())
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Client de diagnostic du pont MCP MT5")
    parser.add_argument("tool", nargs="?", help="outil à appeler ; sans argument, liste les outils")
    parser.add_argument("params", nargs="*", help="paramètres sous la forme clé=valeur")
    parser.add_argument("--url", default=os.environ.get("MCP_MT5_URL", DEFAULT_URL))
    parser.add_argument("--token", default=os.environ.get("MCP_MT5_TOKEN", ""))
    args = parser.parse_args()

    client = Client(args.url, args.token)
    client.handshake()

    if not args.tool:
        reply = client.post({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        tools = (reply or {}).get("result", {}).get("tools", [])
        print(f"{len(tools)} outils\n")
        for tool in tools:
            summary = (tool.get("description") or "").strip().splitlines()
            print(f"- {tool['name']}: {summary[0] if summary else ''}")
        return 0

    arguments = {}
    for item in args.params:
        if "=" not in item:
            print(f"paramètre mal formé : '{item}' (attendu clé=valeur)", file=sys.stderr)
            return 2
        key, value = item.split("=", 1)
        arguments[key] = value

    reply = client.post({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": args.tool, "arguments": arguments},
    }) or {}

    if "error" in reply:
        print(json.dumps(reply["error"], indent=2, ensure_ascii=False))
        return 1

    result = reply.get("result", {})
    # FastMCP returns the value twice: once as text for models that cannot
    # read structured output, once parsed. Prefer the parsed form.
    payload = result.get("structuredContent")
    if payload is None:
        blocks = result.get("content") or []
        text = blocks[0].get("text", "") if blocks else ""
        try:
            payload = json.loads(text)
        except json.JSONDecodeError:
            payload = text

    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 1 if result.get("isError") else 0


if __name__ == "__main__":
    raise SystemExit(main())
