# Clean MCP Mux AppData for fresh start and migrations
# This removes the database and any cached data

$AppName = "com.mcpmux.app"
$AppDataPath = Join-Path $env:LOCALAPPDATA $AppName

Write-Host "MCP Mux AppData Cleaner" -ForegroundColor Cyan
Write-Host "=======================" -ForegroundColor Cyan
Write-Host ""

if (Test-Path $AppDataPath) {
    Write-Host "Found AppData at: $AppDataPath" -ForegroundColor Yellow
    
    # List contents
    Write-Host ""
    Write-Host "Contents:" -ForegroundColor Gray
    Get-ChildItem $AppDataPath -Recurse | ForEach-Object {
        $relativePath = $_.FullName.Replace($AppDataPath, "")
        Write-Host "  $relativePath" -ForegroundColor Gray
    }
    
    Write-Host ""
    $confirm = Read-Host "Delete all contents? (Y/n)"

    if ($confirm -eq '' -or $confirm -eq 'y' -or $confirm -eq 'Y') {
        # Check for running MCP Mux processes
        $mcpmuxProcesses = Get-Process -Name "MCP Mux*", "mcpmux*" -ErrorAction SilentlyContinue
        if ($mcpmuxProcesses) {
            Write-Host ""
            Write-Host "⚠ MCP Mux is running! Processes found:" -ForegroundColor Yellow
            $mcpmuxProcesses | ForEach-Object { Write-Host "  - $($_.ProcessName) (PID: $($_.Id))" -ForegroundColor Yellow }
            Write-Host ""
            $killConfirm = Read-Host "Kill these processes? (Y/n)"
            if ($killConfirm -eq '' -or $killConfirm -eq 'y' -or $killConfirm -eq 'Y') {
                $mcpmuxProcesses | Stop-Process -Force
                Write-Host "  Processes terminated." -ForegroundColor Gray
                Start-Sleep -Seconds 1  # Give OS time to release file handles
            }
            else {
                Write-Host ""
                Write-Host "Cancelled. Please close MCP Mux first." -ForegroundColor Yellow
                return
            }
        }
        
        try {
            Remove-Item $AppDataPath -Recurse -Force -ErrorAction Stop
            Write-Host ""
            Write-Host "✓ AppData cleaned successfully!" -ForegroundColor Green
            Write-Host "  Next app launch will run fresh migrations." -ForegroundColor Gray
        }
        catch {
            Write-Host ""
            Write-Host "✗ Failed to clean AppData: $_" -ForegroundColor Red
            Write-Host "  Make sure MCP Mux is not running." -ForegroundColor Yellow
        }
    }
    else {
        Write-Host ""
        Write-Host "Cancelled." -ForegroundColor Yellow
    }
}
else {
    Write-Host "No AppData found at: $AppDataPath" -ForegroundColor Yellow
    Write-Host "Nothing to clean." -ForegroundColor Gray
}

Write-Host ""
