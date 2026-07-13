# ADR-001: Trading Journal App — System Architecture

**Status:** Proposed  
**Date:** June 2026  
**Deciders:** Chris (solo project)

---

## Context

Build a personal iOS trading journal app (iPhone + iPad) inspired by TradeZella, self-hosted, with AI coaching, a real-time macro terminal, and MT5 integration via FusionMarkets (funded account). Stack: React Native + Rust backend, hosted on Proxmox or Raspberry Pi. Single user, private, no App Store publishing.

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     iOS App (React Native)                  │
│  Dashboard │ Journal │ Analytics │ Macro Terminal │ Coach   │
└────────────────────────┬────────────────────────────────────┘
                         │ HTTPS / REST + WebSocket
                         │
┌────────────────────────▼────────────────────────────────────┐
│              Rust Backend API (axum)                        │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │  Trades  │  │   News   │  │    AI    │  │  Prop     │  │
│  │  Service │  │Aggregator│  │  Engine  │  │  Firm     │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └───────────┘  │
│       │             │             │                         │
│  ┌────▼─────────────▼─────────────▼────────────────────┐   │
│  │              SQLite / PostgreSQL                     │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
         │                    │                    │
         │                    │                    │
┌────────▼──────┐  ┌──────────▼──────┐  ┌─────────▼───────┐
│  MT5 via      │  │  External APIs  │  │  Anthropic API  │
│  FusionMarkets│  │  (news, prices) │  │  (AI Coach)     │
│  EA + CSV     │  │  Investing.com  │  │  claude-sonnet  │
└───────────────┘  │  Yahoo Finance  │  └─────────────────┘
                   │  FXStreet RSS   │
                   │  Bloomberg RSS  │
                   └─────────────────┘
```

---

## Components

### 1. iOS App — React Native

**Navigation structure:**
```
Tab Bar:
├── 🏠 Dashboard        (configurable widgets)
├── 📋 Journal          (trade list + detail)
├── 📊 Analytics        (charts + stats)
├── 🌍 Macro Terminal   (news + prices + calendar)
└── 🤖 AI Coach         (insights + daily report)

Modal / Sheets:
├── Trade Detail        (notes, screenshots, tags)
├── Prop Firm Tracker   (rules + progress)
├── Mood Tracker        (pre-session check-in)
└── Settings
```

**Key libraries:**
- `react-navigation` — navigation
- `react-native-charts-wrapper` ou `Victory Native` — charts
- `react-native-keychain` — Face ID / Touch ID
- `@react-native-async-storage` — offline cache
- `socket.io-client` — WebSocket pour données temps réel
- `react-native-push-notification` — alertes push locales

---

### 2. Rust Backend (axum)

**API Endpoints:**

```
Auth
  POST   /auth/verify-token

Trades
  GET    /trades                    → list with filters
  GET    /trades/:id                → trade detail
  POST   /trades                    → manual entry
  POST   /trades/import/csv         → CSV import
  PUT    /trades/:id                → update (notes, tags, screenshots)
  GET    /trades/stats              → analytics aggregated

Prop Firm
  GET    /accounts                  → list funded accounts
  POST   /accounts                  → add account
  GET    /accounts/:id/rules        → prop firm rules + current status
  PUT    /accounts/:id/rules        → update rules

Macro Terminal
  GET    /macro/prices              → current prices (indices, FX, Or, Oil, BTC)
  GET    /macro/calendar            → eco events (red/orange only)
  GET    /macro/news                → aggregated news feed
  WS     /macro/stream              → WebSocket real-time prices

AI Coach
  GET    /ai/daily-report           → daily session report
  POST   /ai/analyze-trade/:id      → post-trade coaching questions
  GET    /ai/insights               → pattern detection
  POST   /ai/mood                   → mood check-in pre-session

System
  POST   /backup/export             → manual backup trigger
  GET    /health
```

**Rust crates:**
```toml
axum = "0.7"
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.7", features = ["sqlite", "runtime-tokio"] }
serde = { version = "1", features = ["derive"] }
reqwest = { version = "0.11", features = ["json"] }
tower-http = { version = "0.4", features = ["cors"] }
jsonwebtoken = "9"
scraper = "0.17"          # HTML scraping pour news
feed-rs = "1"             # RSS parsing
tokio-tungstenite = "0.20" # WebSocket
```

---

### 3. Database — SQLite (dev) → PostgreSQL (prod)

**Schema principal:**

```sql
-- Comptes de trading
CREATE TABLE accounts (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  broker      TEXT DEFAULT 'FusionMarkets',
  balance     REAL,
  currency    TEXT DEFAULT 'USD',
  is_funded   BOOLEAN DEFAULT true,
  created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Règles prop firm
CREATE TABLE prop_rules (
  id                    TEXT PRIMARY KEY,
  account_id            TEXT REFERENCES accounts(id),
  daily_drawdown_max    REAL,    -- ex: 0.05 = 5%
  global_drawdown_max   REAL,    -- ex: 0.10 = 10%
  profit_target         REAL,
  min_trading_days      INTEGER,
  consistency_rule_pct  REAL DEFAULT 0.20,
  lot_size_max          REAL
);

-- Trades
CREATE TABLE trades (
  id              TEXT PRIMARY KEY,
  account_id      TEXT REFERENCES accounts(id),
  symbol          TEXT NOT NULL,        -- ex: GER40, EURUSD
  direction       TEXT,                 -- LONG / SHORT
  open_time       DATETIME,
  close_time      DATETIME,
  open_price      REAL,
  close_price     REAL,
  lot_size        REAL,
  pnl             REAL,
  pnl_pct         REAL,
  commission      REAL,
  swap            REAL,
  setup_tag       TEXT,                 -- ex: "Break & Retest"
  emotion_tag     TEXT,                 -- ex: "Confident", "FOMO"
  notes           TEXT,
  screenshot_url  TEXT,
  mt5_ticket      TEXT UNIQUE,          -- ID MT5 pour éviter doublons
  created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Sessions (journée de trading)
CREATE TABLE sessions (
  id          TEXT PRIMARY KEY,
  account_id  TEXT REFERENCES accounts(id),
  date        DATE NOT NULL,
  mood_pre    INTEGER,    -- 1-5 avant session
  mood_post   INTEGER,    -- 1-5 après session
  notes       TEXT,
  ai_report   TEXT,       -- rapport IA généré
  created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- News cache
CREATE TABLE news_cache (
  id          TEXT PRIMARY KEY,
  source      TEXT,
  title       TEXT,
  url         TEXT,
  summary     TEXT,
  impact      INTEGER,    -- 1-5 score IA
  sentiment   TEXT,       -- bullish/bearish/neutral
  instruments TEXT,       -- JSON array ex: ["DAX","EUR/USD"]
  published_at DATETIME,
  cached_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Événements économiques
CREATE TABLE eco_events (
  id          TEXT PRIMARY KEY,
  title       TEXT,
  country     TEXT,
  currency    TEXT,
  impact      TEXT,       -- red / orange
  scheduled_at DATETIME,
  actual      TEXT,
  forecast    TEXT,
  previous    TEXT
);
```

---

### 4. MT5 Integration

**Deux modes :**

**Mode A — EA MQL5 (auto-sync)**
```
MT5 Terminal
  └── EA "JournalSync.mq5"
        ├── OnTrade() hook → détecte chaque trade fermé
        └── HTTP POST → http://TON_SERVEUR:8080/trades
              body: { ticket, symbol, direction, open/close, pnl... }
```

**Mode B — CSV Import (fallback)**
```
MT5 → Account History → Export CSV
  └── App iOS → "Import CSV" button
        └── POST /trades/import/csv
              → parsing Rust → deduplication via mt5_ticket
```

---

### 5. Macro Terminal — Data Sources

| Actif | Source gratuite | Fallback payant |
|-------|----------------|-----------------|
| Indices (DAX, S&P...) | Yahoo Finance API | Polygon.io |
| Forex (EUR/USD...) | ExchangeRate-API | OANDA API |
| Or / Pétrole | Yahoo Finance | Quandl |
| BTC | CoinGecko API (free) | — |
| Obligations | FRED API (Fed, gratuit) | — |
| Calendrier éco | Investing.com scraping | Tradingeconomics API |
| News | RSS Bloomberg, FXStreet, Yahoo Finance | NewsAPI.org |

**Refresh strategy:**
- Prices : WebSocket ou polling toutes les 5s
- Calendar : refresh toutes les heures
- News : refresh toutes les 15min

---

### 6. AI Engine — Anthropic API

**Trois usages :**

**A. Post-trade coaching**
```
Input:  trade details + context (session mood, recent P&L)
Prompt: "Act as a trading coach. Ask 3 targeted questions 
         about this trade to help the trader reflect."
Output: 3 coaching questions → affichées dans l'app
```

**B. Daily session report**
```
Input:  all trades of the day + mood + eco events of the day
Prompt: "Generate a concise trading session report:
         performance summary, behavioral patterns detected,
         1 key lesson, 1 thing to improve tomorrow."
Output: rapport markdown → stocké en DB → push notification
```

**C. Pattern detection (weekly)**
```
Input:  last 30 trades + metadata
Prompt: "Analyze these trades. Identify: winning patterns,
         losing patterns, emotional triggers, best/worst 
         instruments, best/worst time of day."
Output: insights structurés en JSON → dashboard Analytics
```

---

### 7. Infrastructure — Self-hosted

**Option A — Proxmox (recommandé)**
```
VM ou LXC Container:
  OS: Ubuntu 24 LTS
  RAM: 512MB suffit
  CPU: 1 core
  Storage: 10GB
  
Services:
  └── trading-journal-api (Rust binary, systemd service)
  └── Nginx (reverse proxy + SSL via Let's Encrypt)
  └── SQLite ou PostgreSQL
```

**Option B — Raspberry Pi 4 (4GB)**
```
OS: Raspberry Pi OS Lite (64-bit)
  └── même setup que Proxmox
  └── Avantage: toujours allumé, faible conso
  └── Inconvénient: moins de RAM pour futures extensions
```

**Accès depuis iPhone (hors LAN) :**
- Tailscale (VPN mesh, gratuit, très simple) ← recommandé
- ou WireGuard (plus technique)
- ou DynDNS + port forwarding (moins sécurisé)

---

## Security

- **Auth** : JWT token stocké dans iOS Keychain, Face ID / Touch ID pour déverrouiller
- **Transport** : HTTPS only (Let's Encrypt via Nginx)
- **VPN** : Tailscale pour accès externe sécurisé
- **API Keys** : stockées en variables d'environnement côté serveur (jamais dans l'app)
- **Backup** : export chiffré manuel vers NAS/Proxmox

---

## Trade-off Analysis

| Décision | Option choisie | Alternative | Raison |
|----------|---------------|-------------|--------|
| Backend lang | Rust (axum) | Node.js / Go | Cohérent avec plan K8s/Rust, perf, apprentissage |
| DB | SQLite → PostgreSQL | MongoDB | Données relationnelles, simple à migrer |
| Mobile | React Native | SwiftUI natif | Cross-platform (iPad+iPhone), JS connu |
| Hosting | Self-hosted Proxmox | VPS cloud | 0€/mois, données privées, déjà l'infra |
| AI | Anthropic API | LLM local (Ollama) | Qualité supérieure, RPi trop limité pour LLM |
| Sync externe | Tailscale | Port forwarding | Sécurité, simplicité, gratuit |

---

## Phased Rollout

### Phase 1 — MVP (Journal de base)
- [ ] Backend Rust : CRUD trades + auth JWT
- [ ] Import CSV MT5
- [ ] App iOS : journal + liste trades + stats basiques
- [ ] Face ID / Touch ID
- [ ] Deploy sur Proxmox + Tailscale

### Phase 2 — Analytics + Prop Firm
- [ ] Stats avancées (win rate, R:R, drawdown, par instrument/heure)
- [ ] Prop Firm tracker + alertes push
- [ ] Tags & setups
- [ ] Notes + screenshots

### Phase 3 — AI Coach
- [ ] Post-trade coaching questions
- [ ] Daily session report
- [ ] Mood tracker
- [ ] Pattern detection

### Phase 4 — Macro Terminal
- [ ] Prix temps réel (WebSocket)
- [ ] Calendrier éco (red/orange)
- [ ] News aggregator + analyse IA
- [ ] Widget iOS

### Phase 5 — Polish
- [ ] Multi-comptes
- [ ] EA MT5 auto-sync
- [ ] Dark/Light toggle
- [ ] Dashboard configurable
- [ ] Backup manuel

---

## What to Revisit Later

- Migration SQLite → PostgreSQL si données > 1GB
- WebSocket natif MT5 si EA HTTP trop instable
- LLM local (Ollama sur Proxmox) si coûts Anthropic API augmentent
- Publication App Store si projet évolue vers multi-users
