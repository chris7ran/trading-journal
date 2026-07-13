# Trading Journal — projet

App de journal de trading personnel, self-hosted (iPhone + iPad), inspirée de
TradeZella : journal des trades, analytics, terminal macro et coach IA. Mono-utilisateur,
privé, sans publication App Store.

## Stack

| Couche | Choix |
|--------|-------|
| Frontend | React Native (iPhone + iPad) |
| Backend | Rust (axum) + SQLite |
| Hébergement | Proxmox ou Raspberry Pi (self-hosted) |
| Accès externe | Tailscale (VPN mesh) |
| IA | Anthropic API |

## Arborescence du dépôt

```
NEW/
├── README.md            ← ce fichier (index projet)
├── ARCHITECTURE.md      ← ADR-001 : architecture système complète
├── code/
│   ├── backend/         ← API Rust/axum (voir code/backend/README.md)
│   └── mobile/          ← app React Native/Expo (voir code/mobile/README.md)
├── sprints/
│   ├── sprint-01.md     ← backend MVP (décisions, écarts, à-faire)
│   └── sprint-02.md     ← app mobile (premier jet, test iPhone)
├── decisions/           ← (à venir) ADRs détaillés par décision
└── concepts/            ← (à venir) notes de conception
```

## Où regarder selon le besoin

- **Comprendre l'architecture** → `ARCHITECTURE.md`
- **Lancer / déployer le backend** → `code/backend/README.md` puis `code/backend/deploy/README.md`
- **Référence API** → tableau des endpoints dans `code/backend/README.md`
- **Avancement & décisions** → `sprints/sprint-01.md`

## État d'avancement (roadmap)

Voir le détail des phases dans `ARCHITECTURE.md` (section *Phased Rollout*).

| Phase | Contenu | Statut |
|-------|---------|--------|
| 1 — MVP backend | Auth JWT, CRUD trades, import CSV MT5, deploy | ✅ code livré (à compiler sur cible) |
| 1 — extension | `/accounts` + règles prop firm, `/trades/stats` | ✅ code livré |
| 1 — app iOS | React Native/Expo : login JWT, liste trades, stats | ✅ premier jet livré (testable via Expo Go) |
| 2 | Analytics avancées, prop firm tracker, tags, screenshots | ⏳ |
| 3 | AI Coach (coaching post-trade, rapport quotidien, mood) | ⏳ |
| 4 | Macro Terminal (prix temps réel, calendrier éco, news) | ⏳ |
| 5 | Polish (multi-comptes, EA MT5 auto-sync, dashboard) | ⏳ |

## Démarrage rapide (backend)

```bash
cd code/backend
cp .env.example .env
# renseigner JWT_SECRET et ADMIN_PASSWORD_HASH (cf. README backend)
cargo run            # API sur 0.0.0.0:8080
cargo test           # tests unitaires + intégration
```

## Conventions

- Code commenté **en anglais** ; doc projet et sprints **en français**.
- Chaque sprint est documenté dans `sprints/sprint-NN.md`.
- Les écarts par rapport à `ARCHITECTURE.md` sont tracés dans le sprint concerné.
