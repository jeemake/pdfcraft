<#
.SYNOPSIS
  Remove Revue PDF K&D: its files, shortcuts and Windows registration.

.DESCRIPTION
  Your PDFs and the app's settings (in %APPDATA%\PdfCraft) are left alone.

.EXAMPLE
  .\Uninstall-Revue.ps1
  .\Uninstall-Revue.ps1 -AllUsers
#>
param(
  # Remove an installation made for every account (needs administrator rights).
  [switch] $AllUsers,
  [switch] $Quiet
)
$ErrorActionPreference = 'Stop'

$AppName = 'Revue PDF K&D'
$AppId = 'RevuePDFKD'
$ExeName = 'RevuePDF.exe'
$ProgId = "$AppId.Document"

if ($AllUsers) {
  $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-AllUsers')
    if ($Quiet) { $arguments += '-Quiet' }
    Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -Wait
    return
  }
  $Hive = 'HKLM:'
  $InstallDir = Join-Path $env:ProgramFiles $AppName
  $StartMenu = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
  $Desktop = [Environment]::GetFolderPath('CommonDesktopDirectory')
} else {
  $Hive = 'HKCU:'
  $InstallDir = Join-Path $env:LOCALAPPDATA "Programs\$AppName"
  $StartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
  $Desktop = [Environment]::GetFolderPath('Desktop')
}
$Exe = Join-Path $InstallDir $ExeName

$running = Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($ExeName)) -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and ($_.Path -ieq $Exe) }
if ($running) {
  if ($Quiet) { throw "$AppName is running. Close it, then uninstall again." }
  Read-Host "$AppName is open. Save your work and close it, then press Enter to continue" | Out-Null
}

function Remove-Key([string] $Key) {
  $path = "$Hive\Software\$Key"
  if (Test-Path $path) { Remove-Item -Path $path -Recurse -Force }
}
function Remove-Value([string] $Key, [string] $Name) {
  $path = "$Hive\Software\$Key"
  if (Test-Path $path) { Remove-ItemProperty -Path $path -Name $Name -ErrorAction SilentlyContinue }
}

Remove-Key "Classes\Applications\$ExeName"
Remove-Key "Classes\$ProgId"
Remove-Value 'Classes\.pdf\OpenWithProgids' $ProgId
Remove-Key "Classes\SystemFileAssociations\.pdf\shell\$AppId.Open"
Remove-Key $AppId
Remove-Value 'RegisteredApplications' $AppId
Remove-Key "Microsoft\Windows\CurrentVersion\App Paths\$ExeName"
Remove-Key "Microsoft\Windows\CurrentVersion\Uninstall\$AppId"

foreach ($folder in @($StartMenu, $Desktop)) {
  $link = Join-Path $folder "$AppName.lnk"
  if (Test-Path $link) { Remove-Item $link -Force }
}

# This script may be running from the folder it removes: delete it after this process ends.
if (Test-Path $InstallDir) {
  $quoted = $InstallDir.Replace("'", "''")
  $later = "Start-Sleep -Seconds 2; Remove-Item -LiteralPath '$quoted' -Recurse -Force -ErrorAction SilentlyContinue"
  Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-WindowStyle', 'Hidden', '-Command', $later) -WindowStyle Hidden
}

Add-Type -Namespace RevueSetup -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int eventId, int flags, System.IntPtr item1, System.IntPtr item2);'
[RevueSetup.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

if (-not $Quiet) { Write-Host "$AppName is uninstalled. Your PDFs and settings were kept." }
