# Builds the Windows installer: release exe + shipped python runtime -> Inno Setup.
# Usage: pwsh scripts/package-windows.ps1 [-Arch x64|arm64] [-Out <dir>] [-Iscc <path>] [-SkipBuild]
# Needs: rustup, bun, Inno Setup 6 (ISCC.exe on PATH or in %LOCALAPPDATA%\Programs\Inno Setup 6 / Program Files).
param(
  [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
  [string]$Out,
  [string]$Iscc,
  [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$desktop = Split-Path $PSScriptRoot -Parent
$repo = Split-Path (Split-Path $desktop -Parent) -Parent
$triple = if ($Arch -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
if (-not $Out) { $Out = Join-Path $desktop "dist" }
$stage = Join-Path $Out "stage-windows-$Arch"
$version = ((Select-String -Path (Join-Path $desktop 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value)

Push-Location $desktop
try {
  # the dev runtime keeps libs (needed to link); the stage below drops it
  bun scripts/setup.ts --triple $triple
  if ($LASTEXITCODE) { throw 'setup failed' }
  if (-not $SkipBuild) {
    cargo build -p ytdlpd --release --locked --target $triple
    if ($LASTEXITCODE) { throw 'cargo build failed' }
  }
} finally { Pop-Location }

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $desktop 'target' }
$rel = Join-Path $target "$triple\release"
if (-not (Test-Path (Join-Path $rel 'ytdlpd.exe'))) { $rel = Join-Path $target 'release' } # host build without --target
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item (Join-Path $rel 'ytdlpd.exe') $stage
Get-ChildItem $rel -Filter '*.dll' | Where-Object { $_.Name -match '^(python|vcruntime)' } | Copy-Item -Destination $stage
Copy-Item (Join-Path $desktop "runtime\$triple\python") (Join-Path $stage 'python') -Recurse
Remove-Item (Join-Path $stage 'python\libs') -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $repo 'THIRD-PARTY.md'), (Join-Path $repo 'LICENSE') $stage

# the staged tree must run on its own (no python on PATH needed)
& (Join-Path $stage 'ytdlpd.exe') --self-test
if ($LASTEXITCODE) { throw 'self-test of the staged tree failed' }

if (-not $Iscc) {
  $Iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
  if (-not $Iscc) {
    $Iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
  }
}
if (-not $Iscc) { throw 'ISCC.exe not found: install Inno Setup 6 or pass -Iscc' }
& $Iscc "/DStage=$stage" "/DVersion=$version" "/DArch=$Arch" "/DOut=$Out" (Join-Path $PSScriptRoot 'inno\ytdlpd.iss')
if ($LASTEXITCODE) { throw 'ISCC failed' }
Write-Host "installer: $(Join-Path $Out "ytdlpd-$version-windows-$Arch-setup.exe")"
