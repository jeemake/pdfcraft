<#
.SYNOPSIS
  Build Revue PDF K&D and make its setup: dist\Revue-PDF-KD-Setup.zip.

.DESCRIPTION
  Run this once, on any Windows 10 or 11 PC, from a checkout of this repository. It checks for the
  build tools (Rust and the Visual C++ build tools) and offers to install what is missing with
  winget, builds the app, and puts it in a setup folder with Install.cmd and Uninstall.cmd.

  Copy the zip to the touchscreen PC, extract it, and double-click Install.cmd. That PC needs no
  build tools.

  The first build takes 15 to 30 minutes; later ones are much quicker.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\windows\revue\Build-RevueSetup.ps1
  powershell -ExecutionPolicy Bypass -File packaging\windows\revue\Build-RevueSetup.ps1 -WithOcr
#>
param(
  # Also bundle the text-recognition (OCR) models, for scanned PDFs (downloads about 30 MB).
  [switch] $WithOcr,
  # Install missing build tools without asking.
  [switch] $Yes
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$Here = $PSScriptRoot

function Invoke-Native([string] $What, [scriptblock] $Block) {
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed (exit code $LASTEXITCODE)" }
}

function Confirm-Install([string] $What) {
  if ($Yes) { return $true }
  $answer = Read-Host "$What is missing. Install it now with winget? [Y/n]"
  return ($answer -eq '' -or $answer -match '^[Yy]')
}

function Update-Path {
  $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
  $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
  if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }
}

# ---- build tools --------------------------------------------------------------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$hasVc = (Test-Path $vswhere) -and (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $hasVc) {
  if (-not (Get-Command winget -ErrorAction SilentlyContinue)) { throw 'The Visual C++ build tools are missing and winget is not available. Install "Build Tools for Visual Studio 2022" with the "Desktop development with C++" workload, then run this again.' }
  if (-not (Confirm-Install 'The Visual C++ build tools')) { throw 'The Visual C++ build tools are needed to build the app.' }
  Invoke-Native 'Installing the Visual C++ build tools' {
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements `
      --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
  }
}
Update-Path
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  if (-not (Get-Command winget -ErrorAction SilentlyContinue)) { throw 'Rust is missing and winget is not available. Install Rust from https://rustup.rs, then run this again.' }
  if (-not (Confirm-Install 'Rust')) { throw 'Rust is needed to build the app.' }
  Invoke-Native 'Installing Rust' { winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements }
  Update-Path
  Invoke-Native 'Setting up Rust' { rustup default stable-x86_64-pc-windows-msvc }
}

# ---- build --------------------------------------------------------------------------------------
$Target = 'x86_64-pc-windows-msvc'
Invoke-Native 'Adding the Windows target' { rustup target add $Target }
# A static C runtime: the touchscreen PC needs no Visual C++ redistributable.
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:PDFCRAFT_REQUIRE_WINRES = '1'
Write-Host 'Building Revue PDF K&D (the first build takes a while)...'
Push-Location $Root
try {
  Invoke-Native 'cargo build' { cargo build --release --locked -p pdfcraft --target $Target }
} finally { Pop-Location }
$Built = Join-Path $Root "target\$Target\release\pdfcraft.exe"
if (-not (Test-Path $Built)) { throw "the build finished without $Built" }

# ---- setup folder -------------------------------------------------------------------------------
$Dist = Join-Path $Root 'dist'
$Setup = Join-Path $Dist 'Revue-PDF-KD-Setup'
$App = Join-Path $Setup 'app'
Remove-Item -Recurse -Force $Setup -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $App | Out-Null
Copy-Item $Built (Join-Path $App 'RevuePDF.exe')
# The licences the app and its icons are distributed under.
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') { Copy-Item (Join-Path $Root $f) $App }
Copy-Item (Join-Path $Root 'assets\app-icon\LICENSE-document.txt') $App
if ($env:CRAFT_FONTS_DIR) {
  # Fonts embedded from craft-fonts carry their OFL notices.
  . (Join-Path $Root 'packaging\windows\font-notices.ps1')
  $fonts = Join-Path $App 'font-licences'
  New-Item -ItemType Directory -Force -Path $fonts | Out-Null
  foreach ($ofl in @(Get-CraftFontNotices $env:CRAFT_FONTS_DIR $false)) {
    Copy-Item -LiteralPath $ofl.FullName -Destination (Join-Path $fonts "OFL-$($ofl.Directory.Name).txt")
  }
}
if ($WithOcr) {
  $models = Join-Path $App 'models'
  New-Item -ItemType Directory -Force -Path $models | Out-Null
  Push-Location $Root
  try { Invoke-Native 'cargo xtask models' { cargo xtask models $models } } finally { Pop-Location }
  Remove-Item -Force (Join-Path $models '*.part') -ErrorAction SilentlyContinue
}
$version = '1.0.0'
foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
  if ($line -match '^\s*version\s*=\s*"([^"]+)"') { $version = $Matches[1]; break }
}
Set-Content -Path (Join-Path $App 'VERSION.txt') -Value $version -Encoding ASCII

foreach ($f in 'Install-Revue.ps1', 'Uninstall-Revue.ps1', 'Install.cmd', 'Uninstall.cmd', 'README.txt') {
  Copy-Item (Join-Path $Here $f) $Setup
}

$Zip = Join-Path $Dist 'Revue-PDF-KD-Setup.zip'
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Setup -DestinationPath $Zip
Write-Host ''
Write-Host "Done: $Zip"
Write-Host 'Copy it to the touchscreen PC, extract it, and double-click Install.cmd.'
