# McpMux Development Environment Setup
# Run once after cloning the repo

param(
    [switch]$SkipRust,
    [switch]$SkipNode,
    [switch]$SkipPlaywright,
    [switch]$SkipTauriDriver
)

Write-Host "=== McpMux Dev Setup ===" -ForegroundColor Cyan

# Check prerequisites
Write-Host "`nChecking prerequisites..." -ForegroundColor Yellow

$hasNode = $null -ne (Get-Command "node" -ErrorAction SilentlyContinue)
$hasPnpm = $null -ne (Get-Command "pnpm" -ErrorAction SilentlyContinue)
$hasCargo = $null -ne (Get-Command "cargo" -ErrorAction SilentlyContinue)

if (-not $hasNode) { Write-Host "  Missing: Node.js (https://nodejs.org/)" -ForegroundColor Red }
if (-not $hasPnpm) { Write-Host "  Missing: pnpm (npm install -g pnpm)" -ForegroundColor Red }
if (-not $hasCargo) { Write-Host "  Missing: Rust (https://rustup.rs/)" -ForegroundColor Red }

if (-not ($hasNode -and $hasPnpm -and $hasCargo)) {
    Write-Host "`nPlease install the missing tools and run this script again." -ForegroundColor Red
    exit 1
}

Write-Host "  Node.js: $(node --version)" -ForegroundColor Green
Write-Host "  pnpm: $(pnpm --version)" -ForegroundColor Green
Write-Host "  Rust: $(rustc --version)" -ForegroundColor Green

# Install Node dependencies
if (-not $SkipNode) {
    Write-Host "`nInstalling Node dependencies..." -ForegroundColor Yellow
    pnpm install
    Write-Host "  Done" -ForegroundColor Green
}

# Install Playwright browsers using npx (more compatible)
if (-not $SkipPlaywright) {
    Write-Host "`nInstalling Playwright browsers..." -ForegroundColor Yellow
    npx playwright install chromium
    Write-Host "  Done" -ForegroundColor Green
}

# Install tauri-driver for full E2E tests
if (-not $SkipTauriDriver) {
    Write-Host "`nInstalling tauri-driver..." -ForegroundColor Yellow
    cargo install tauri-driver --locked
    Write-Host "  Done" -ForegroundColor Green
    
    # Windows: Install Edge WebDriver (required for tauri-driver)
    if ($IsWindows -or $env:OS -eq "Windows_NT") {
        Write-Host "`nInstalling Edge WebDriver..." -ForegroundColor Yellow
        
        # Get Edge version
        $edgePath = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
        if (-not (Test-Path $edgePath)) {
            $edgePath = "C:\Program Files\Microsoft\Edge\Application\msedge.exe"
        }
        
        if (Test-Path $edgePath) {
            $edgeVersion = (Get-Item $edgePath).VersionInfo.ProductVersion
            $majorVersion = $edgeVersion.Split('.')[0]
            Write-Host "  Edge version: $edgeVersion" -ForegroundColor Cyan
            
            # Download matching Edge WebDriver
            $driverUrl = "https://msedgedriver.azureedge.net/$edgeVersion/edgedriver_win64.zip"
            $driverZip = "$env:TEMP\edgedriver.zip"
            $driverDir = "$env:LOCALAPPDATA\EdgeDriver"
            
            try {
                Write-Host "  Downloading Edge WebDriver..." -ForegroundColor Cyan
                Invoke-WebRequest -Uri $driverUrl -OutFile $driverZip -UseBasicParsing
                
                # Extract
                if (Test-Path $driverDir) { Remove-Item -Recurse -Force $driverDir }
                Expand-Archive -Path $driverZip -DestinationPath $driverDir -Force
                Remove-Item $driverZip
                
                # Add to PATH permanently (user level)
                $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
                if ($currentPath -notlike "*$driverDir*") {
                    [Environment]::SetEnvironmentVariable("PATH", "$driverDir;$currentPath", "User")
                    Write-Host "  Added to user PATH permanently" -ForegroundColor Green
                }
                
                # Also add to current session
                $env:PATH = "$driverDir;$env:PATH"
                
                Write-Host "  Edge WebDriver installed to: $driverDir" -ForegroundColor Green
            } catch {
                Write-Host "  Warning: Could not download Edge WebDriver" -ForegroundColor Yellow
                Write-Host "  Download manually from: https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/" -ForegroundColor Yellow
            }
        } else {
            Write-Host "  Warning: Edge not found. Tauri E2E tests won't work." -ForegroundColor Yellow
        }
    }
}

# Install cargo-nextest for faster Rust tests
if (-not $SkipRust) {
    Write-Host "`nInstalling cargo-nextest..." -ForegroundColor Yellow
    cargo install cargo-nextest --locked
    Write-Host "  Done" -ForegroundColor Green
}

Write-Host "`n=== Setup Complete ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "You can now run:"
Write-Host "  pnpm dev           - Start development server"
Write-Host "  pnpm test          - Run all tests (Rust + TypeScript)"
Write-Host "  pnpm test:e2e:web  - Run web E2E tests (Playwright) - RECOMMENDED"
Write-Host ""
Write-Host "Full Tauri E2E (advanced, requires build):"
Write-Host "  pnpm build         - Build the app first"
Write-Host "  pnpm test:e2e      - Run Tauri E2E tests"
Write-Host ""
