# McpMux Development Environment Setup
# Run once after cloning the repo

param(
    [switch]$SkipRust,
    [switch]$SkipNode,
    [switch]$SkipPlaywright,
    [switch]$SkipTauriDriver
)

$ErrorActionPreference = "Stop"

Write-Host "=== McpMux Dev Setup ===" -ForegroundColor Cyan

# Check prerequisites
Write-Host "`nChecking prerequisites..." -ForegroundColor Yellow

$missing = @()

if (-not (Get-Command "node" -ErrorAction SilentlyContinue)) {
    $missing += "Node.js (https://nodejs.org/)"
}

if (-not (Get-Command "pnpm" -ErrorAction SilentlyContinue)) {
    $missing += "pnpm (npm install -g pnpm)"
}

if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    $missing += "Rust (https://rustup.rs/)"
}

if ($missing.Count -gt 0) {
    Write-Host "`nMissing prerequisites:" -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host "`nPlease install the missing tools and run this script again." -ForegroundColor Red
    exit 1
}

Write-Host "  Node.js: $(node --version)" -ForegroundColor Green
Write-Host "  pnpm: $(pnpm --version)" -ForegroundColor Green
Write-Host "  Rust: $(rustc --version)" -ForegroundColor Green

# Install Node dependencies
if (-not $SkipNode) {
    Write-Host "`nInstalling Node dependencies..." -ForegroundColor Yellow
    pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Failed to install Node dependencies" -ForegroundColor Red
        exit 1
    }
    Write-Host "  Node dependencies installed" -ForegroundColor Green
}

# Install Playwright browsers
if (-not $SkipPlaywright) {
    Write-Host "`nInstalling Playwright browsers..." -ForegroundColor Yellow
    pnpm exec playwright install --with-deps chromium
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Failed to install Playwright browsers" -ForegroundColor Red
        exit 1
    }
    Write-Host "  Playwright browsers installed" -ForegroundColor Green
}

# Install tauri-driver for full E2E tests
if (-not $SkipTauriDriver) {
    Write-Host "`nInstalling tauri-driver..." -ForegroundColor Yellow
    cargo install tauri-driver --locked
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Failed to install tauri-driver" -ForegroundColor Red
        exit 1
    }
    Write-Host "  tauri-driver installed" -ForegroundColor Green
}

# Install cargo-nextest for faster Rust tests
if (-not $SkipRust) {
    Write-Host "`nInstalling cargo-nextest..." -ForegroundColor Yellow
    cargo install cargo-nextest --locked
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Warning: Failed to install cargo-nextest (tests will still work with cargo test)" -ForegroundColor Yellow
    } else {
        Write-Host "  cargo-nextest installed" -ForegroundColor Green
    }
}

Write-Host "`n=== Setup Complete ===" -ForegroundColor Cyan
Write-Host @"

You can now run:
  pnpm dev           - Start development server
  pnpm test          - Run all tests
  pnpm test:e2e:web  - Run web E2E tests (Playwright)
  pnpm test:e2e      - Run full Tauri E2E tests (requires built app)

"@ -ForegroundColor White
