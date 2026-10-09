<#
.SYNOPSIS
  Build and package SolveCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    solvecraft-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    solvecraft-<version>-windows-<arch>-portable.zip   solvecraft.exe + solvecraft-cli.exe + licences
    SolveCraft-<version>-windows-<arch>.exe            the app alone (x64 only: a plain download)

  The binaries link the C runtime statically (+crt-static), so nothing else needs installing.
  The icon comes from $env:SOLVECRAFT_ICONS (solvecraft.ico, made by `solvecraft-cli icon`), or
  is drawn by the CLI just built when this machine can run it. Nothing is signed.

  Needs: Rust (MSVC toolchain + the target) and WiX v5:
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

  Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:SOLVECRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Output "SolveCraft $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p solvecraft -p solvecraft-cli --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"

# Check both binaries' PE headers: Machine must match -Arch; Subsystem 2 = GUI (the app opens no
# console window), 3 = console (the CLI's output reaches the terminal).
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('solvecraft.exe', 2), @('solvecraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'solvecraft.exe'), (Join-Path $Bin 'solvecraft-cli.exe') $Stage
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt') { Copy-Item (Join-Path $Root $f) $Stage }

$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
$CanRun = ($Arch -ne 'arm64' -or $HostArch -eq 'arm64')
$Icon = if ($env:SOLVECRAFT_ICONS) { Join-Path $env:SOLVECRAFT_ICONS 'solvecraft.ico' } else { Join-Path $Stage 'solvecraft.ico' }
if (-not (Test-Path $Icon)) {
  if (-not $CanRun) { throw "no icon: set SOLVECRAFT_ICONS to a folder with solvecraft.ico (an $Arch CLI can't run on this $HostArch machine)" }
  Invoke-Native 'draw the icon' { & (Join-Path $Stage 'solvecraft-cli.exe') icon --out $Icon }
}

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "solvecraft-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'solvecraft.wxs') -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $PSScriptRoot 'solvecraft.en-us.wxl') `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" -d "IconPath=$Icon" `
    -o $Msi
}
Invoke-Native 'MSI shortcut icon validation (ICE50)' {
  wix msi validate $Msi -ice ICE50 -intermediateFolder (Join-Path $Stage 'msi-validation')
}
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\solvecraft-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt') { Copy-Item (Join-Path $Root $f) $Portable }
$Zip = Join-Path $Dist "solvecraft-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# ---- the app as a plain download (x64) ---------------------------------------------------------
$Outputs = @($Msi, $Zip)
if ($Arch -eq 'x64') {
  $Exe = Join-Path $Dist "SolveCraft-$Version-windows-$Arch.exe"
  Copy-Item (Join-Path $Stage 'solvecraft.exe') $Exe
  $Outputs += $Exe
}

if ($CanRun) {
  Invoke-Native 'solvecraft-cli --version' { & (Join-Path $Stage 'solvecraft-cli.exe') --version }
} else {
  Write-Output "skipping solvecraft-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Outputs | Format-Table Name, Length
