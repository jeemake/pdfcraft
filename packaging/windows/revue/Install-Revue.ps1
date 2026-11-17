<#
.SYNOPSIS
  Install Revue PDF K&D from this setup folder.

.DESCRIPTION
  Copies RevuePDF.exe into Programs, adds Start menu and desktop shortcuts, and registers the
  app with Windows. It then appears under "Open with" for PDF files, as "Open with Revue PDF K&D"
  on the PDF right-click menu, in Settings > Default apps, and in Settings > Apps for uninstalling.

  It never takes over the PDF default by itself: -MakeDefault opens the Settings page where you
  choose it (Windows only lets a person make that choice).

  Without -AllUsers it installs for the current account and needs no administrator rights.

.EXAMPLE
  .\Install-Revue.ps1
  .\Install-Revue.ps1 -AllUsers -NoDesktopShortcut
  .\Install-Revue.ps1 -Quiet           # no questions, for remote deployment
#>
param(
  # Install for every account on this PC (needs administrator rights).
  [switch] $AllUsers,
  [switch] $NoDesktopShortcut,
  # Open Settings > Default apps afterwards to make Revue the app for PDF files.
  [switch] $MakeDefault,
  # Ask nothing and open nothing.
  [switch] $Quiet
)
$ErrorActionPreference = 'Stop'

$AppName = 'Revue PDF K&D'
$AppId = 'RevuePDFKD'
$ExeName = 'RevuePDF.exe'
$ProgId = "$AppId.Document"
$Here = $PSScriptRoot
$Payload = Join-Path $Here 'app'

function Say([string] $Text) { if (-not $Quiet) { Write-Host $Text } }

if (-not (Test-Path (Join-Path $Payload $ExeName))) {
  throw "This setup folder has no app\$ExeName. Build it first with Build-RevueSetup.ps1, or extract the whole zip before running Install."
}

if ($AllUsers) {
  $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    # Start again elevated, with the same choices.
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-AllUsers')
    if ($NoDesktopShortcut) { $arguments += '-NoDesktopShortcut' }
    if ($MakeDefault) { $arguments += '-MakeDefault' }
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

# A running copy holds its files: ask before closing it, so nothing unsaved is lost.
$running = Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($ExeName)) -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and ($_.Path -ieq $Exe) }
if ($running) {
  if ($Quiet) { throw "$AppName is running. Close it, then install again." }
  Read-Host "$AppName is open. Save your work and close it, then press Enter to continue" | Out-Null
  Start-Sleep -Seconds 1
  if (Get-Process -Id $running.Id -ErrorAction SilentlyContinue) { throw "$AppName is still running. Close it and install again." }
}

Say "Installing $AppName to $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Path (Join-Path $Payload '*') -Destination $InstallDir -Recurse -Force
Copy-Item -Path (Join-Path $Here 'Uninstall-Revue.ps1') -Destination $InstallDir -Force

function Set-Value([string] $Key, [string] $Name, $Value, [string] $Type = 'String') {
  $path = "$Hive\Software\$Key"
  if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
  if ($Name -eq '') {
    Set-Item -Path $path -Value $Value
  } else {
    New-ItemProperty -Path $path -Name $Name -Value $Value -PropertyType $Type -Force | Out-Null
  }
}

$open = "`"$Exe`" `"%1`""
$icon = "`"$Exe`",0"
$docIcon = "`"$Exe`",-2"

# The app itself: its name under "Open with", and what it opens.
Set-Value "Classes\Applications\$ExeName" 'FriendlyAppName' $AppName
Set-Value "Classes\Applications\$ExeName\DefaultIcon" '' $docIcon
Set-Value "Classes\Applications\$ExeName\SupportedTypes" '.pdf' ''
Set-Value "Classes\Applications\$ExeName\shell\open\command" '' $open
# A file type of its own, offered for .pdf without replacing the current default.
Set-Value "Classes\$ProgId" '' 'PDF Document'
Set-Value "Classes\$ProgId" 'FriendlyTypeName' 'PDF Document'
Set-Value "Classes\$ProgId\DefaultIcon" '' $docIcon
Set-Value "Classes\$ProgId\shell\open" 'FriendlyAppName' $AppName
Set-Value "Classes\$ProgId\shell\open\command" '' $open
Set-Value 'Classes\.pdf\OpenWithProgids' $ProgId ''
# "Open with Revue PDF K&D" on the PDF right-click menu (Windows 11: under Show more options).
Set-Value "Classes\SystemFileAssociations\.pdf\shell\$AppId.Open" '' "Open with $AppName"
Set-Value "Classes\SystemFileAssociations\.pdf\shell\$AppId.Open" 'Icon' $icon
Set-Value "Classes\SystemFileAssociations\.pdf\shell\$AppId.Open" 'MultiSelectModel' 'Document'
Set-Value "Classes\SystemFileAssociations\.pdf\shell\$AppId.Open\command" '' $open
# Settings > Default apps.
Set-Value "$AppId\Capabilities" 'ApplicationName' $AppName
Set-Value "$AppId\Capabilities" 'ApplicationDescription' 'PDF drawing board for plans'
Set-Value "$AppId\Capabilities" 'ApplicationIcon' $icon
Set-Value "$AppId\Capabilities\FileAssociations" '.pdf' $ProgId
Set-Value 'RegisteredApplications' $AppId "Software\$AppId\Capabilities"
Set-Value "Microsoft\Windows\CurrentVersion\App Paths\$ExeName" '' $Exe

# Settings > Apps: uninstall from there.
$version = '1.0.0'
$versionFile = Join-Path $Payload 'VERSION.txt'
if (Test-Path $versionFile) { $version = (Get-Content $versionFile -TotalCount 1).Trim() }
$scope = ''
if ($AllUsers) { $scope = ' -AllUsers' }
$uninstall = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$InstallDir\Uninstall-Revue.ps1`"$scope"
$size = [int]((Get-ChildItem $InstallDir -Recurse -File | Measure-Object -Property Length -Sum).Sum / 1KB)
$u = "Microsoft\Windows\CurrentVersion\Uninstall\$AppId"
Set-Value $u 'DisplayName' $AppName
Set-Value $u 'DisplayVersion' $version
Set-Value $u 'Publisher' 'K&D'
Set-Value $u 'DisplayIcon' $icon
Set-Value $u 'InstallLocation' $InstallDir
Set-Value $u 'UninstallString' $uninstall
Set-Value $u 'QuietUninstallString' "$uninstall -Quiet"
Set-Value $u 'EstimatedSize' $size 'DWord'
Set-Value $u 'NoModify' 1 'DWord'
Set-Value $u 'NoRepair' 1 'DWord'

# Shortcuts.
$shell = New-Object -ComObject WScript.Shell
function New-Shortcut([string] $Folder) {
  $link = $shell.CreateShortcut((Join-Path $Folder "$AppName.lnk"))
  $link.TargetPath = $Exe
  $link.WorkingDirectory = $InstallDir
  $link.IconLocation = "$Exe,0"
  $link.Description = 'PDF drawing board for plans'
  $link.Save()
}
New-Shortcut $StartMenu
if (-not $NoDesktopShortcut) { New-Shortcut $Desktop }

# Tell Explorer the file associations changed, so "Open with" shows the app at once.
Add-Type -Namespace RevueSetup -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int eventId, int flags, System.IntPtr item1, System.IntPtr item2);'
[RevueSetup.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Say ''
Say "$AppName is installed."
Say '  - Start it from the Start menu or the desktop.'
Say "  - Right-click a PDF > Open with > $AppName."
if ($MakeDefault -and -not $Quiet) {
  Say "  - In the window that opens, choose $AppName for .pdf files."
  Start-Process "ms-settings:defaultapps?registeredAppUser=$AppId"
} elseif (-not $Quiet) {
  Say "  - To open every PDF with it: right-click a PDF > Open with > Choose another app > $AppName, and tick Always."
}
