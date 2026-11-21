# Windows Win32 Desktop Protocol End-to-End Test Harness
# Launches screentimed on Windows with win32 hooks, spawns real
# Win32 desktop applications (notepad.exe, powershell.exe), switches
# foreground focus, and asserts SQLite database output.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WorkspaceRoot = (Resolve-Path "$ScriptDir\..\..").Path

# Locate screentimed executable
$ScreentimedBin = $null
if (Test-Path "$WorkspaceRoot\target\release\screentimed.exe") {
    $ScreentimedBin = "$WorkspaceRoot\target\release\screentimed.exe"
} elseif (Test-Path "$WorkspaceRoot\target\debug\screentimed.exe") {
    $ScreentimedBin = "$WorkspaceRoot\target\debug\screentimed.exe"
} else {
    Write-Host "screentimed.exe not found. Building release binary..."
    Push-Location $WorkspaceRoot
    try {
        cargo build --release --bin screentimed
        $ScreentimedBin = "$WorkspaceRoot\target\release\screentimed.exe"
    } finally {
        Pop-Location
    }
}

Write-Host "Using screentimed binary: $ScreentimedBin"

# Create isolated temporary directory
$TestGuid = [Guid]::NewGuid().ToString("N")
$TestDir = Join-Path $env:TEMP "screentime-win32-e2e-$TestGuid"
New-Item -ItemType Directory -Force -Path $TestDir | Out-Null

$DbPath = Join-Path $TestDir "test_screentime.db"
$SocketPath = Join-Path $TestDir "screentime.sock"
$LogPath = Join-Path $TestDir "screentimed.log"

# Define Win32 Helper for foreground window manipulation
Add-Type @"
using System;
using System.Runtime.InteropServices;

public class Win32Helper {
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
}
"@

$DaemonProcess = $null
$App1Process = $null
$App2Process = $null

function Cleanup {
    Write-Host "Cleaning up Windows E2E test resources..."
    if ($App1Process -and -not $App1Process.HasExited) {
        Stop-Process -Id $App1Process.Id -Force -ErrorAction SilentlyContinue
    }
    if ($App2Process -and -not $App2Process.HasExited) {
        Stop-Process -Id $App2Process.Id -Force -ErrorAction SilentlyContinue
    }
    if ($DaemonProcess -and -not $DaemonProcess.HasExited) {
        Stop-Process -Id $DaemonProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path $TestDir) {
        Remove-Item -Recurse -Force $TestDir -ErrorAction SilentlyContinue
    }
}

try {
    Write-Host "Starting screentimed with windows backend..."
    $DaemonProcess = Start-Process `
        -FilePath $ScreentimedBin `
        -ArgumentList "--platform windows --db `"$DbPath`" --socket `"$SocketPath`" --log-console --log-level debug" `
        -RedirectStandardOutput $LogPath `
        -RedirectStandardError (Join-Path $TestDir "screentimed.err") `
        -PassThru

    Write-Host "Waiting for screentimed to initialize (PID: $($DaemonProcess.Id))..."
    $DaemonReady = $false
    for ($i = 0; $i -lt 30; $i++) {
        $statusProcess = Start-Process `
            -FilePath $ScreentimedBin `
            -ArgumentList "status --socket `"$SocketPath`"" `
            -Wait `
            -PassThru `
            -NoNewWindow
        if ($statusProcess.ExitCode -eq 0) {
            $DaemonReady = $true
            break
        }
        Start-Sleep -Milliseconds 500
    }

    if (-not $DaemonReady) {
        Write-Error "screentimed failed to initialize within 15 seconds"
        if (Test-Path $LogPath) { Get-Content $LogPath }
        exit 1
    }
    Write-Host "screentimed is running and active on IPC socket."

    # Launch real Win32 test processes
    Write-Host "Launching notepad.exe..."
    $App1Process = Start-Process notepad.exe -PassThru
    Start-Sleep -Seconds 2

    if ($App1Process.MainWindowHandle -ne [IntPtr]::Zero) {
        [Win32Helper]::ShowWindow($App1Process.MainWindowHandle, 9) | Out-Null
        [Win32Helper]::SetForegroundWindow($App1Process.MainWindowHandle) | Out-Null
    }
    Start-Sleep -Seconds 3

    Write-Host "Launching secondary process (cmd.exe)..."
    $App2Process = Start-Process cmd.exe -ArgumentList "/k title ScreentimeTestCmd" -PassThru
    Start-Sleep -Seconds 2

    if ($App2Process.MainWindowHandle -ne [IntPtr]::Zero) {
        [Win32Helper]::ShowWindow($App2Process.MainWindowHandle, 9) | Out-Null
        [Win32Helper]::SetForegroundWindow($App2Process.MainWindowHandle) | Out-Null
    }
    Start-Sleep -Seconds 3

    # Switch back to Notepad
    if ($App1Process.MainWindowHandle -ne [IntPtr]::Zero) {
        Write-Host "Switching focus back to notepad.exe..."
        [Win32Helper]::SetForegroundWindow($App1Process.MainWindowHandle) | Out-Null
    }
    Start-Sleep -Seconds 3

    # Close test apps
    Stop-Process -Id $App1Process.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $App2Process.Id -Force -ErrorAction SilentlyContinue

    # Terminate screentimed
    Write-Host "Stopping screentimed..."
    Stop-Process -Id $DaemonProcess.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1

    # Assert SQLite rows using Python
    Write-Host "Asserting SQLite database records in $DbPath..."
    python - $DbPath @"
import sqlite3, sys

db_path = sys.argv[1]
conn = sqlite3.connect(db_path)
cur = conn.cursor()

rows = cur.execute(
    "SELECT id, start_time, end_time, app_class, app_title FROM sessions ORDER BY id ASC"
).fetchall()

print(f"Total recorded sessions in {db_path}: {len(rows)}")
for r in rows:
    duration = r[2] - r[1]
    print(f"  ID={r[0]}: class='{r[3]}', title='{r[4]}', duration={duration:.2f}s")

if len(rows) < 1:
    print(f"ERROR: Expected at least 1 session, found {len(rows)}", file=sys.stderr)
    sys.exit(1)

classes = [str(r[3]).lower() for r in rows]
has_target = any("notepad" in c or "cmd" in c or "powershell" in c for c in classes)

if not has_target:
    print(f"ERROR: Expected test application in recorded classes: {classes}", file=sys.stderr)
    sys.exit(1)

for r in rows:
    if r[1] <= 0 or r[2] < r[1]:
        print(f"ERROR: Invalid session timestamp: start={r[1]}, end={r[2]}", file=sys.stderr)
        sys.exit(1)

print("SUCCESS: Windows Win32 E2E assertions passed!")
"@

    if ($LASTEXITCODE -ne 0) {
        Write-Error "SQLite assertions failed"
        exit 1
    }

    Write-Host "Windows Win32 E2E test finished successfully."
} finally {
    Cleanup
}
