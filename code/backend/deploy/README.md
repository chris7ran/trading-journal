# Deployment — Proxmox + systemd + Tailscale

Two supported paths: a **native systemd service** (recommended for a 512 MB
LXC/VM) and a **Docker container**. Both expose the API on port `8080`, reached
from your iPhone over **Tailscale**.

---

## 0. Provision the host (Proxmox)

Create an Ubuntu 24.04 LXC container or VM (1 vCPU, 512 MB RAM, 10 GB disk),
then SSH in.

```bash
sudo apt update && sudo apt install -y curl build-essential pkg-config
```

---

## Path A — Native binary + systemd (recommended)

### A1. Build the release binary

On a build machine (or the host itself):

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

cd backend
cargo build --release
# Output: target/release/trading-journal-api
```

> ARM note: building directly on a Raspberry Pi works but is slow. You can also
> cross-compile or just build on the Pi once.

### A2. Create user, directories, and install files

```bash
sudo useradd --system --home /var/lib/trading-journal-api --shell /usr/sbin/nologin tjapi
sudo mkdir -p /opt/trading-journal-api /etc/trading-journal-api /var/lib/trading-journal-api

sudo cp target/release/trading-journal-api /opt/trading-journal-api/
sudo cp deploy/trading-journal-api.service /etc/systemd/system/
sudo chown -R tjapi:tjapi /var/lib/trading-journal-api
```

### A3. Configure secrets

```bash
# Generate a JWT secret
JWT_SECRET=$(openssl rand -hex 32)

# Generate the password hash (interactive)
PW_HASH=$(/opt/trading-journal-api/trading-journal-api hash 'YOUR-PASSWORD' | head -n1)

sudo tee /etc/trading-journal-api/api.env >/dev/null <<EOF
JWT_SECRET=$JWT_SECRET
ADMIN_PASSWORD_HASH='$PW_HASH'
DATABASE_URL=sqlite:///var/lib/trading-journal-api/journal.db
BIND_ADDR=0.0.0.0:8080
JWT_TTL_HOURS=168
CORS_ALLOWED_ORIGINS=*
RUST_LOG=info,sqlx=warn
# Optional: enables monthly US indicators + real Fed Funds (free key).
FRED_API_KEY=
EOF

sudo chmod 600 /etc/trading-journal-api/api.env
sudo chown tjapi:tjapi /etc/trading-journal-api/api.env
```

### A4. Start the service

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now trading-journal-api
sudo systemctl status trading-journal-api
journalctl -u trading-journal-api -f      # follow logs
```

### A5. Verify

```bash
curl -s localhost:8080/health
# {"status":"ok","service":"trading-journal-api","version":"0.1.0"}
```

---

## Path B — Docker

```bash
cd backend
docker build -t trading-journal-api:0.1.0 .

# Create .env from .env.example, fill JWT_SECRET + ADMIN_PASSWORD_HASH
docker run -d --name tjapi \
  --restart unless-stopped \
  -p 8080:8080 \
  --env-file .env \
  -v tjapi-data:/app/data \
  trading-journal-api:0.1.0

# Generate a password hash with the same image:
docker run --rm trading-journal-api:0.1.0 hash 'YOUR-PASSWORD'
```

### Path B-bis — Docker Compose (simplest, recommended for Docker)

```bash
cd backend
cp .env.example .env          # fill JWT_SECRET, ADMIN_PASSWORD_HASH, optional FRED_API_KEY
docker compose up -d --build  # builds + starts, restarts on boot
docker compose logs -f
docker compose ps             # STATUS shows "healthy" once /health responds
```

The `tjapi-data` named volume keeps the database across `docker compose up --build`
rebuilds. Update = `git pull && docker compose up -d --build`.

---

## Automated backups

`deploy/backup.sh` takes a **consistent SQLite snapshot** (safe while the API
runs), gzips it, and rotates (keeps the newest 14).

Native (systemd timer, daily at 03:30):

```bash
sudo cp deploy/backup.sh /opt/trading-journal-api/ && sudo chmod +x /opt/trading-journal-api/backup.sh
sudo cp deploy/trading-journal-backup.service deploy/trading-journal-backup.timer /etc/systemd/system/
sudo apt install -y sqlite3        # for the consistent .backup snapshot
sudo systemctl daemon-reload && sudo systemctl enable --now trading-journal-backup.timer
systemctl list-timers | grep trading-journal   # verify next run
/opt/trading-journal-api/backup.sh              # run once now to test
```

Docker: point the script at the volume mount and cron it on the host, e.g.
`TJ_DB=/var/lib/docker/volumes/backend_tjapi-data/_data/journal.db backup.sh`,
or run `docker compose exec api sqlite3 /app/data/journal.db ".backup '/app/data/backup.db'"`
and copy it out. Store copies off-box (e.g. `rclone`/`scp`) for real safety.

---

## CI

`.github/workflows/ci.yml` runs on push/PR: builds + tests the Rust backend
(hard gate) and type-checks the mobile app (`tsc --noEmit`); `cargo fmt`/`clippy`
run as informative steps. Push the repo to GitHub to activate it.

---

## Tailscale (access from iPhone outside the LAN)

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
```

After `tailscale up`, the host gets a stable name like
`proxmox-journal.tailXXXX.ts.net`. From the iPhone (with the Tailscale app
connected to the same tailnet), the API is reachable at:

```
http://proxmox-journal.tailXXXX.ts.net:8080
```

For a single-user private setup, **Tailscale alone (no public exposure) is the
simplest secure option**: the WireGuard tunnel already encrypts all traffic, so
plain `http://…:8080` between iPhone and server is safe. No public port, no
public certificate needed.

---

## HTTPS over Tailscale (optional — only for a real iOS build)

> In **Expo Go today you don't need this**: plain HTTP over Tailscale already
> works and is encrypted by the tunnel. But a **production / dev build** enforces
> iOS App Transport Security (ATS), which blocks plain HTTP. Two private options,
> **no public port**:

### Option 1 — Real cert on your `*.ts.net` name (recommended)

In the Tailscale admin console: **DNS → enable MagicDNS + HTTPS Certificates**.
Then on the server:

```bash
# Issue a Let's Encrypt cert for the machine (DNS-validated, nothing exposed).
sudo tailscale cert <host>.<tailnet>.ts.net

# Terminate TLS on 443 and proxy to the local API on 8080.
sudo tailscale serve --bg --https=443 http://127.0.0.1:8080
sudo tailscale serve status
```

In the app, set the server URL to `https://<host>.<tailnet>.ts.net` (no port).
Only devices on your tailnet can reach it. Disable with
`sudo tailscale serve --https=443 off`.

### Option 2 — Allow HTTP for your tailnet host (quicker, personal use)

Add an ATS exception in `mobile/app.json`:

```json
"ios": {
  "infoPlist": {
    "NSAppTransportSecurity": {
      "NSExceptionDomains": {
        "<host>.<tailnet>.ts.net": { "NSExceptionAllowsInsecureHTTPLoads": true }
      }
    }
  }
}
```

The app can then use `http://<host>.<tailnet>.ts.net:8080`. Fine for a personal /
TestFlight build; Option 1 is cleaner if you want a valid certificate.

---

## Smoke test the full flow

```bash
BASE=http://localhost:8080

# 1. Login -> token
TOKEN=$(curl -s -X POST $BASE/auth/login \
  -H 'content-type: application/json' \
  -d '{"password":"YOUR-PASSWORD"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')

# 2. Create a trade
curl -s -X POST $BASE/trades \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"symbol":"GER40","direction":"LONG","pnl":120.5}'

# 3. List trades
curl -s $BASE/trades -H "authorization: Bearer $TOKEN"

# 4. Import an MT5 CSV export
curl -s -X POST "$BASE/trades/import/csv" \
  -H "authorization: Bearer $TOKEN" -H 'content-type: text/csv' \
  --data-binary @history.csv
```

---

## Updating

Native: rebuild, copy the binary over, `sudo systemctl restart trading-journal-api`.
Docker: rebuild the image, `docker stop/rm tjapi`, run again (the `tjapi-data`
volume preserves the database).
