# Sprint 28 — Infra : compose, sauvegardes, CI

**Date:** 2026-07-06
**Objectif:** compléter l'infra de déploiement au-delà du Dockerfile/systemd/Tailscale
déjà présents.
**Statut:** livré (fichiers de conf + doc). À activer côté serveur/GitHub.

---

## Ajouts

- **`code/backend/docker-compose.yml`** : déploiement reproductible en une commande
  (`docker compose up -d --build`), `restart: unless-stopped`, volume `tjapi-data`
  persistant, `env_file: .env`, **healthcheck** (bash `/dev/tcp` → pas d'outil en plus).
- **Sauvegardes automatiques** :
  - `deploy/backup.sh` : snapshot SQLite **consistant** (`sqlite3 .backup`, sûr même
    en WAL), gzip + rotation (garde 14). Chemins configurables par env.
  - `deploy/trading-journal-backup.service` + `.timer` : sauvegarde **quotidienne**
    (03:30, `Persistent=true` pour rattraper si la machine était éteinte).
- **CI** — `.github/workflows/ci.yml` : sur push/PR, **build + test** du backend Rust
  (gate dur), `tsc --noEmit` du mobile ; `cargo fmt`/`clippy` en informatif.
- **`.env.example`** : ajout de `FRED_API_KEY` (optionnel).
- **`deploy/README.md`** : nouvelles sections Docker Compose, Sauvegardes, CI +
  `FRED_API_KEY` dans l'exemple systemd.

## À faire côté Chris

```bash
# Docker Compose
cd code/backend && cp .env.example .env   # remplir secrets (+ FRED_API_KEY)
docker compose up -d --build

# Sauvegardes (natif systemd)
sudo cp deploy/backup.sh /opt/trading-journal-api/ && sudo chmod +x /opt/trading-journal-api/backup.sh
sudo cp deploy/trading-journal-backup.{service,timer} /etc/systemd/system/
sudo apt install -y sqlite3
sudo systemctl enable --now trading-journal-backup.timer

# CI : pousser le repo sur GitHub (workflow auto-détecté)
```

## Note

Ces fichiers sont de la configuration (pas de compilation ici). YAML/bash/unit
relus manuellement. Le healthcheck compose suppose l'image debian-slim (bash
présent). Stocker les backups hors-machine (scp/rclone) pour une vraie résilience.

## Reste (roadmap infra/app)

- [ ] Dev build iOS (Face ID réel) + icône/splash + éventuel TestFlight.
- [ ] Sauvegarde off-site (rclone) + restauration testée.
- [ ] Notifications push.
