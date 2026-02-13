# McpMux Release Setup Script
#
# This script helps set up the release signing infrastructure.
# Run once to generate signing keys for Tauri auto-updater.
#
# Usage:
#   .\setup-release.ps1              # Generate new signing key
#   .\setup-release.ps1 -ShowPubkey  # Show public key for tauri.conf.json

param(
    [switch]$ShowPubkey
)

$keyPath = "$env:USERPROFILE\.tauri\mcpmux.key"
$pubkeyPath = "$env:USERPROFILE\.tauri\mcpmux.key.pub"

if ($ShowPubkey) {
    if (Test-Path $pubkeyPath) {
        Write-Host ""
        Write-Host "Public key for tauri.conf.json:" -ForegroundColor Cyan
        Write-Host ""
        Get-Content $pubkeyPath
        Write-Host ""
    } else {
        Write-Host "No public key found. Run without -ShowPubkey to generate." -ForegroundColor Yellow
    }
    exit 0
}

Write-Host ""
Write-Host "McpMux Release Setup" -ForegroundColor Cyan
Write-Host "====================" -ForegroundColor Cyan
Write-Host ""

# Check if key already exists
if (Test-Path $keyPath) {
    Write-Host "Signing key already exists at: $keyPath" -ForegroundColor Yellow
    Write-Host ""
    $response = Read-Host "Overwrite? (y/N)"
    if ($response -ne 'y' -and $response -ne 'Y') {
        Write-Host "Aborted." -ForegroundColor Gray
        exit 0
    }
}

# Create directory if needed
$keyDir = Split-Path $keyPath
if (-not (Test-Path $keyDir)) {
    New-Item -ItemType Directory -Path $keyDir -Force | Out-Null
}

Write-Host "Generating Tauri signing key..." -ForegroundColor Gray
Write-Host ""

# Generate the key
Push-Location (Join-Path $PSScriptRoot "..")
try {
    pnpm --filter @mcpmux/desktop tauri signer generate -w $keyPath
    
    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "Success!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Files created:" -ForegroundColor Gray
        Write-Host "  Private key: $keyPath" -ForegroundColor White
        Write-Host "  Public key:  $pubkeyPath" -ForegroundColor White
        Write-Host ""
        Write-Host "Next steps:" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "1. Copy the PUBLIC key to tauri.conf.json:" -ForegroundColor Gray
        Write-Host "   .\setup-release.ps1 -ShowPubkey" -ForegroundColor White
        Write-Host ""
        Write-Host "2. Add the PRIVATE key to GitHub secrets:" -ForegroundColor Gray
        Write-Host "   - Go to: https://github.com/ion-ash/mcp-mux/settings/secrets/actions" -ForegroundColor White
        Write-Host "   - Add secret: TAURI_SIGNING_PRIVATE_KEY" -ForegroundColor White
        Write-Host "   - Value: contents of $keyPath" -ForegroundColor White
        Write-Host ""
        Write-Host "3. (Optional) Add password to GitHub secrets:" -ForegroundColor Gray
        Write-Host "   - Add secret: TAURI_SIGNING_PRIVATE_KEY_PASSWORD" -ForegroundColor White
        Write-Host ""
    }
} finally {
    Pop-Location
}
