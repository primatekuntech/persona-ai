set dotenv-load

# ── Dev ───────────────────────────────────────────────────────────────────────

# Start the full local stack (postgres + backend + frontend dev server)
up:
    docker compose up -d db
    sleep 2
    cd backend && cargo run

# Start only postgres
db:
    docker compose up -d db

# Run backend tests (requires running postgres)
test:
    cd backend && cargo test

# Run backend tests with output
test-verbose:
    cd backend && cargo test -- --nocapture

# Clippy lint
lint:
    cd backend && cargo clippy --all-targets -- -D warnings

# Format check
fmt:
    cd backend && cargo fmt --check

# ── Database ──────────────────────────────────────────────────────────────────

# Apply migrations manually
migrate:
    cd backend && cargo sqlx migrate run

# Revert last migration
migrate-revert:
    cd backend && cargo sqlx migrate revert

# Prepare sqlx offline cache (run after schema changes)
prepare:
    cd backend && cargo sqlx prepare

# ── Frontend ─────────────────────────────────────────────────────────────────

# Install frontend deps
frontend-install:
    cd frontend && npm ci

# Start frontend dev server
frontend-dev:
    cd frontend && npm run dev

# Build frontend for production
frontend-build:
    cd frontend && npm run build

# ── Docker ───────────────────────────────────────────────────────────────────

# Build all Docker images
docker-build:
    docker compose build

# Production stack (requires certs/ and filled .env)
prod-up:
    docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d

prod-down:
    docker compose -f docker-compose.yml -f docker-compose.prod.yml down

# ── Utilities ────────────────────────────────────────────────────────────────

# Generate a new SESSION_SECRET
gen-secret:
    openssl rand -hex 32

# Download model files (see scripts/download-models.sh)
download-models:
    bash scripts/download-models.sh
