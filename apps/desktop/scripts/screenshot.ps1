# Starts the debug build with the QA data dir, waits for the `ready` log line, captures the window
# to <ShotDir>\<Name>.png (PrintWindow with PW_RENDERFULLCONTENT, CopyFromScreen when that is blank),
# then stops the app.
#   pwsh scripts/screenshot.ps1 -Name downloads
#   pwsh scripts/screenshot.ps1 -Name busy --demo busy      (extra arguments go to ytdlpd)
param(
    [string]$Name = 'shot',
    [string]$Exe = '',
    [string]$DataDir = 'F:\tmp\ytdlpd-rs\qa-data',
    [string]$ShotDir = 'F:\tmp\ytdlpd-rs\shots',
    [string]$Size = '1280x800',
    [int]$TimeoutSeconds = 60,
    [int]$SettleMs = 1500,
    [Parameter(ValueFromRemainingArguments = $true)] [string[]]$Extra
)
$ErrorActionPreference = 'Stop'
if (-not $Exe) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path (Split-Path $PSScriptRoot -Parent) 'target' }
    $Exe = Join-Path $target 'debug\ytdlpd.exe'
}
if (-not (Test-Path $Exe)) { throw "cannot find $Exe (cargo build -p ytdlpd first)" }
New-Item -ItemType Directory -Force $ShotDir, $DataDir | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
[void][Win]::SetProcessDPIAware()

$log = Join-Path $ShotDir "$Name.log"
$argList = @('--data-dir', $DataDir, '--window-size', $Size) + $Extra
$env:RUST_LOG = 'ytdlpd=info'
$p = Start-Process -FilePath $Exe -ArgumentList $argList -PassThru -RedirectStandardError $log -RedirectStandardOutput "$log.out"
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not ((Test-Path $log) -and (Select-String -Path $log -Pattern 'ytdlpd: ready' -Quiet))) {
        if ($p.HasExited) { throw "ytdlpd exited early (code $($p.ExitCode)); see $log" }
        if ((Get-Date) -gt $deadline) { throw "no ready line after $TimeoutSeconds s; see $log" }
        Start-Sleep -Milliseconds 200
    }
    Start-Sleep -Milliseconds $SettleMs
    $p.Refresh()
    while ($p.MainWindowHandle -eq 0) {
        if ((Get-Date) -gt $deadline) { throw 'no main window' }
        Start-Sleep -Milliseconds 200; $p.Refresh()
    }
    $h = $p.MainWindowHandle
    $r = New-Object Win+RECT
    [void][Win]::GetWindowRect($h, [ref]$r)
    $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $dc = $g.GetHdc()
    $ok = [Win]::PrintWindow($h, $dc, 2) # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($dc)
    # a blank capture (every sampled pixel black or transparent) means the window is not composited
    $blank = $true
    foreach ($i in 1..40) {
        $c = $bmp.GetPixel([int]($w * $i / 41), [int]($ht * (($i * 7) % 40 + 1) / 41))
        if (($c.R + $c.G + $c.B) -gt 0 -and $c.A -gt 0) { $blank = $false; break }
    }
    if (-not $ok -or $blank) {
        Write-Host 'PrintWindow came back blank, falling back to CopyFromScreen'
        [void][Win]::SetForegroundWindow($h)
        Start-Sleep -Milliseconds 400
        $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $ht))
    }
    $g.Dispose()
    $out = Join-Path $ShotDir "$Name.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "saved $out (${w}x${ht})"
}
finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
