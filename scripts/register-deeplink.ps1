# MCP Mux Deep Link Registration Script
# 
# Registers the mcpmux:// URL scheme handler for development.
# Run this script once after cloning the repo (no admin required).
#
# Usage:
#   .\register-deeplink.ps1           # Register with default path
#   .\register-deeplink.ps1 -ExePath "path\to\exe"  # Custom exe path
#   .\register-deeplink.ps1 -Unregister            # Remove registration
#   .\register-deeplink.ps1 -Test                  # Test the deep link
#
# The registration is per-user (HKCU), not machine-wide.

param(
    [switch]$Unregister,
    [switch]$Test,
    [string]$ExePath
)

$scheme = "mcpmux"
$regPath = "HKCU:\Software\Classes\$scheme"
$displayName = "MCP Mux"

# Handle unregister
if ($Unregister) {
    if (Test-Path $regPath) {
        Remove-Item -Path $regPath -Recurse -Force
        Write-Host "✓ Unregistered $scheme`:// deep link handler" -ForegroundColor Green
    } else {
        Write-Host "- $scheme`:// was not registered" -ForegroundColor Yellow
    }
    exit 0
}

# Handle test
if ($Test) {
    Write-Host "Testing deep link..." -ForegroundColor Cyan
    Start-Process "$scheme`://test"
    Write-Host "✓ If $displayName opened, the deep link is working!" -ForegroundColor Green
    exit 0
}

# Find the executable
if (-not $ExePath) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $repoRoot = Split-Path -Parent $scriptDir
    
    # Check for debug build first, then release
    $debugExe = Join-Path $repoRoot "target\debug\mcmux-desktop.exe"
    $releaseExe = Join-Path $repoRoot "target\release\mcmux-desktop.exe"
    
    if (Test-Path $debugExe) {
        $ExePath = $debugExe
    } elseif (Test-Path $releaseExe) {
        $ExePath = $releaseExe
    } else {
        Write-Host "✗ Could not find mcmux-desktop.exe" -ForegroundColor Red
        Write-Host "  Run 'cargo build -p mcmux-desktop' first, or specify -ExePath" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "  Usage:" -ForegroundColor Gray
        Write-Host "    .\register-deeplink.ps1              # Auto-find and register" -ForegroundColor White
        Write-Host "    .\register-deeplink.ps1 -ExePath X   # Register specific exe" -ForegroundColor White
        Write-Host "    .\register-deeplink.ps1 -Unregister  # Remove registration" -ForegroundColor White
        Write-Host "    .\register-deeplink.ps1 -Test        # Test deep link" -ForegroundColor White
        exit 1
    }
}

if (-not (Test-Path $ExePath)) {
    Write-Host "✗ Executable not found: $ExePath" -ForegroundColor Red
    exit 1
}

# Resolve to absolute path
$ExePath = (Resolve-Path $ExePath).Path

Write-Host ""
Write-Host "$displayName Deep Link Registration" -ForegroundColor Cyan
Write-Host ("=" * 35) -ForegroundColor Cyan
Write-Host ""
Write-Host "Scheme:     " -NoNewline -ForegroundColor Gray
Write-Host "$scheme`://" -ForegroundColor White
Write-Host "Executable: " -NoNewline -ForegroundColor Gray  
Write-Host "$ExePath" -ForegroundColor White
Write-Host ""

try {
    # Create registry entries for URL protocol handler
    # See: https://docs.microsoft.com/en-us/previous-versions/windows/internet-explorer/ie-developer/platform-apis/aa767914(v=vs.85)
    
    New-Item -Path $regPath -Force | Out-Null
    Set-ItemProperty -Path $regPath -Name '(Default)' -Value "URL:$displayName Protocol"
    Set-ItemProperty -Path $regPath -Name 'URL Protocol' -Value ''
    
    # Optional: Add icon (using the exe's icon)
    New-Item -Path "$regPath\DefaultIcon" -Force | Out-Null
    Set-ItemProperty -Path "$regPath\DefaultIcon" -Name '(Default)' -Value "`"$ExePath`",0"
    
    # Command to execute when URL is opened
    New-Item -Path "$regPath\shell\open\command" -Force | Out-Null
    Set-ItemProperty -Path "$regPath\shell\open\command" -Name '(Default)' -Value "`"$ExePath`" `"%1`""
    
    Write-Host "✓ Registered $scheme`:// deep link handler" -ForegroundColor Green
    Write-Host ""
    Write-Host "Quick test:" -ForegroundColor Gray
    Write-Host "  .\register-deeplink.ps1 -Test" -ForegroundColor White
    Write-Host ""
    Write-Host "Or manually:" -ForegroundColor Gray
    Write-Host "  Start-Process '$scheme`://test'" -ForegroundColor White
    Write-Host ""
    Write-Host "To remove:" -ForegroundColor Gray
    Write-Host "  .\register-deeplink.ps1 -Unregister" -ForegroundColor White
    Write-Host ""
}
catch {
    Write-Host "✗ Failed to register: $_" -ForegroundColor Red
    exit 1
}
