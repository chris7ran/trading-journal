# Sprint 29 — Déploiement homelab : nettoyage + accès privé Tailscale

**Date:** 2026-07-12
**Contexte:** serveur Ubuntu sous Proxmox (Lenovo M720q), Docker déjà en place
(Immich). Objectif : repartir sur une base propre et exposer les services en HTTPS
**privé** via Tailscale (sans Traefik, sans port ouvert).
**Détails techniques :** voir [[infra-tailscale]].

---

## Nettoyage

Supprimé (chacun son compose sauf Portainer, lancé à la main) :
`traefik`, `vaultwarden`, `portainer`, `socket-proxy` (le socket-proxy ne servait
qu'à Traefik). **Immich intact** (`immich_server/postgres/machine-learning/redis`
= Valkey — dépendance d'Immich, à ne jamais supprimer). Réseau `proxy` retiré,
volumes des stacks supprimés, `/opt/{traefik,vaultwarden,socket-proxy}` effacés.

## Réinstallation propre

Stacks rangés dans `~/stacks/` (pas de sudo, Docker tourne en tant qu'utilisateur) :
- **Portainer** : `--http-enabled`, publié `127.0.0.1:9000` (loopback), volume
  `portainer_data`, socket Docker monté.
- **Vaultwarden** : publié `127.0.0.1:8081`, `DOMAIN=https://<host>.<tailnet>.ts.net`,
  `ADMIN_TOKEN` via `env_file`, image passée à `:latest` (fix extension Bitwarden).

## Accès privé (Tailscale)

- Tailscale installé sur le serveur + connecté au tailnet ; **MagicDNS** et
  **HTTPS Certificates** activés dans la console.
- Reverse proxy intégré :
  ```
  tailscale serve --bg --https=443  http://127.0.0.1:8081   # Vaultwarden
  tailscale serve --bg --https=9443 http://127.0.0.1:9000   # Portainer
  ```
- Nom du tailnet **conservé** (`<tailnet>.ts.net`) — le rename est un one-shot et
  purement cosmétique.
- Clients (Mac/iPhone) : app Tailscale installée + connectée pour résoudre `*.ts.net`.

## État final

| Service | URL (tailnet) | Backend local |
|---|---|---|
| Vaultwarden | `https://<host>.<tailnet>.ts.net` | 127.0.0.1:8081 |
| Portainer | `https://<host>.<tailnet>.ts.net:9443` | 127.0.0.1:9000 |
| Immich | `http://192.168.1.x:2283` (LAN, pas encore sur Tailscale) | — |
| Journal (à venir) | `https://<host>.<tailnet>.ts.net:8443` | 127.0.0.1:8090 |

Tout en HTTPS privé, **0 port ouvert**, chiffré WireGuard.

## Reste à faire

- [ ] Backend journal : push GitHub → `git clone` sur le serveur → `docker compose up`
      (port `127.0.0.1:8090:8080`) → `tailscale serve --https=8443`.
- [ ] (Option) exposer Immich via Tailscale aussi.
- [ ] (Option) ACL Tailscale pour restreindre qui accède à quoi.
