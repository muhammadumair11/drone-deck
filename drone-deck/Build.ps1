<#
.SYNOPSIS
    Builds Drone Deck in release mode, deploys it beside the repository root and packages dist\DroneDeck-<version>.

.DESCRIPTION
    Works under Windows PowerShell 5.1 and PowerShell 7. Cargo writes its progress to stderr, so native commands run
    with stderr merged into normal output and success is judged by the exit code only.

    Outputs:
      <repo>\Drone Deck.exe, <repo>\runtime\, <repo>\assets\   local copy for running from the checkout
      <repo>\dist\DroneDeck-<version>\                          clean package (no test scripts, no headless scenarios)
      <repo>\dist\DroneDeck-<version>.zip                       the same package zipped

.PARAMETER NoLocalDeploy
    Skip the copy beside the repository root (used by CI).
#>
[CmdletBinding()]
param(
    [switch]$NoLocalDeploy
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

$project = $PSScriptRoot
$repo = Split-Path $project -Parent

function Fail([string]$message) {
    Write-Host ''
    Write-Host "Build failed: $message" -ForegroundColor Red
    exit 1
}

# Runs a native command without letting stderr become a terminating PowerShell error (Windows PowerShell 5.1 turns
# redirected stderr lines into ErrorRecords). Output is streamed to the host; the exit code is returned.
function Invoke-Native([string]$exe, [string[]]$arguments) {
    $ErrorActionPreference = 'Continue'
    & $exe @arguments 2>&1 | ForEach-Object {
        if ($_ -is [System.Management.Automation.ErrorRecord]) { Write-Host $_.Exception.Message } else { Write-Host "$_" }
    }
    return $LASTEXITCODE
}

function Find-Cargo {
    $onPath = Get-Command cargo -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $candidate = Join-Path $cargoHome 'bin\cargo.exe'
    if (Test-Path -LiteralPath $candidate) {
        $env:PATH = (Split-Path $candidate -Parent) + ';' + $env:PATH
        return $candidate
    }
    Fail ("cargo was not found on PATH or in $candidate.`n" +
        '  Install Rust from https://rustup.rs (choose the default x86_64-pc-windows-msvc host), then open a new terminal.')
}

function Test-MsvcLinker {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $installs = & $vswhere -all -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        foreach ($install in @($installs)) {
            if (-not $install) { continue }
            $linkers = Get-ChildItem -Path (Join-Path $install 'VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe') -ErrorAction SilentlyContinue
            if ($linkers) { return $true }
        }
    }
    $link = Get-Command link.exe -ErrorAction SilentlyContinue
    return [bool]($link -and $link.Source -match '\\MSVC\\')
}

function Test-ResourceCompiler {
    if ($env:RC) { return (Test-Path -LiteralPath $env:RC) }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    if (-not (Test-Path -LiteralPath $kits)) { return $false }
    return [bool](Get-ChildItem -Path (Join-Path $kits '*\x64\rc.exe') -ErrorAction SilentlyContinue)
}

function Get-PackageVersion {
    $manifest = Get-Content -LiteralPath (Join-Path $project 'Cargo.toml') -Raw
    $package = [regex]::Match($manifest, '(?ms)^\[package\](.*?)(^\[|\z)')
    $version = [regex]::Match($package.Groups[1].Value, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $version.Success) { Fail 'Could not read the package version from drone-deck\Cargo.toml.' }
    return $version.Groups[1].Value
}

# Mirrors $source into $destination with robocopy. Exit codes below 8 mean success.
function Copy-Tree([string]$source, [string]$destination, [string[]]$excludeFiles = @()) {
    $arguments = @($source, $destination, '/MIR', '/XD', '__pycache__', '/NFL', '/NDL', '/NJH', '/NJS', '/NP', '/R:2', '/W:1')
    if ($excludeFiles.Count -gt 0) {
        $arguments += '/XF'
        $arguments += $excludeFiles
    }
    $code = Invoke-Native 'robocopy.exe' $arguments
    if ($code -ge 8) { Fail "Copying $source to $destination failed (robocopy exit code $code)." }
    $global:LASTEXITCODE = 0
}

function Copy-Executable([string]$source, [string]$destination) {
    try {
        Copy-Item -LiteralPath $source -Destination $destination -Force
    } catch {
        Fail ("Could not replace $destination. Close Drone Deck if it is running, then build again.`n  $($_.Exception.Message)")
    }
}

# Preflight
if ($env:OS -ne 'Windows_NT') { Fail 'Build.ps1 packages the Windows build and must run on Windows.' }
$cargo = Find-Cargo
if (-not (Test-MsvcLinker)) {
    Fail ("The MSVC linker (link.exe) was not found.`n" +
        '  Install Visual Studio Build Tools with the "Desktop development with C++" workload:' + "`n" +
        '  https://visualstudio.microsoft.com/visual-cpp-build-tools/')
}
if (-not (Test-ResourceCompiler)) {
    Fail ("The Windows SDK resource compiler (rc.exe) was not found; it embeds the app icon.`n" +
        '  Install a Windows 10/11 SDK (included in the "Desktop development with C++" workload), or set RC to rc.exe.')
}
$version = Get-PackageVersion
Write-Host "Drone Deck $version - using $cargo"

# Build. rustup picks the toolchain from rust-toolchain.toml by the working directory, not --manifest-path, so cargo
# runs from the project folder whatever folder the script was started from.
Push-Location -LiteralPath $project
try {
    $code = Invoke-Native $cargo @('build', '--release', '--manifest-path', (Join-Path $project 'Cargo.toml'))
} finally {
    Pop-Location
}
if ($code -ne 0) { Fail "cargo build exited with code $code." }
$exe = Join-Path $project 'target\release\drone-deck.exe'
if (-not (Test-Path -LiteralPath $exe)) { Fail "Expected $exe after the build." }

$assets = Join-Path $project 'assets'
$scripts = Join-Path $project 'scripts'

# Local deploy beside the repository root (ignored by git).
if (-not $NoLocalDeploy) {
    Copy-Executable $exe (Join-Path $repo 'Drone Deck.exe')
    Copy-Tree $assets (Join-Path $repo 'assets')
    Copy-Tree $scripts (Join-Path $repo 'runtime')
    Write-Host "Deployed Drone Deck.exe, runtime\ and assets\ to $repo"
}

# Distribution package
$dist = Join-Path $repo 'dist'
$name = "DroneDeck-$version"
$package = Join-Path $dist $name
$zip = Join-Path $dist "$name.zip"
if (Test-Path -LiteralPath $package) { Remove-Item -LiteralPath $package -Recurse -Force }
New-Item -ItemType Directory -Path $package -Force | Out-Null

Copy-Executable $exe (Join-Path $package 'Drone Deck.exe')
Copy-Tree $assets (Join-Path $package 'assets')
# Test scripts (test_*.py, test-*.ps1, test-*.py) are left out at every depth, as are the icon tools that regenerate and
# check drone-deck\assets at development time; the top-level *.json files are headless UI scenarios, while
# setup\catalog.json is runtime data and stays.
Copy-Tree $scripts (Join-Path $package 'runtime') @('test_*', 'test-*', 'generate-icon.ps1', 'verify-icon.ps1')
Get-ChildItem -LiteralPath (Join-Path $package 'runtime') -Filter '*.json' -File | Remove-Item -Force

# Documentation: drone-deck\docs\*.md plus the top-level drone-deck\*.md files that START-HERE.md or those docs link
# to, all flattened into docs\. Links are rewritten to the flat layout.
$docs = Join-Path $package 'docs'
New-Item -ItemType Directory -Path $docs -Force | Out-Null
$utf8 = New-Object Text.UTF8Encoding $false
$linkPattern = '\]\((?:\.\./|docs/)?([A-Za-z0-9._-]+\.md)'
$start = Get-Content -LiteralPath (Join-Path $project 'START-HERE.md') -Raw -Encoding UTF8
$sources = @{}
$sourceDocs = Join-Path $project 'docs'
if (Test-Path -LiteralPath $sourceDocs) {
    Get-ChildItem -LiteralPath $sourceDocs -Filter '*.md' -File | ForEach-Object { $sources[$_.Name] = $_.FullName }
}
$texts = @($start) + @($sources.Values | ForEach-Object { Get-Content -LiteralPath $_ -Raw -Encoding UTF8 })
while ($texts.Count -gt 0) {
    $found = @()
    foreach ($link in @($texts | ForEach-Object { [regex]::Matches([string]$_, $linkPattern) })) {
        $docName = $link.Groups[1].Value
        $top = Join-Path $project $docName
        if (-not $sources.ContainsKey($docName) -and (Test-Path -LiteralPath $top)) {
            $sources[$docName] = $top
            $found += Get-Content -LiteralPath $top -Raw -Encoding UTF8
        }
    }
    $texts = $found
}
foreach ($docName in $sources.Keys) {
    $text = Get-Content -LiteralPath $sources[$docName] -Raw -Encoding UTF8
    $text = [regex]::Replace($text, '\]\((?:\.\./|docs/)([A-Za-z0-9._-]+\.md)', ']($1')
    [IO.File]::WriteAllText((Join-Path $docs $docName), $text, $utf8)
}
$start = [regex]::Replace($start, '\]\(([A-Za-z0-9._-]+\.md)', '](docs/$1')
[IO.File]::WriteAllText((Join-Path $package 'START HERE.md'), $start, $utf8)
$license = Join-Path $repo 'LICENSE'
if (Test-Path -LiteralPath $license) { Copy-Item -LiteralPath $license -Destination $package -Force }

if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
# Entries are written one by one so names use '/' (.NET Framework's CreateFromDirectory writes '\').
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$base = (Get-Item -LiteralPath $dist).FullName.TrimEnd('\')
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in Get-ChildItem -LiteralPath $package -Recurse -File) {
        $entry = $file.FullName.Substring($base.Length + 1).Replace('\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $entry,
            [IO.Compression.CompressionLevel]::Optimal)
    }
} finally {
    $archive.Dispose()
}

Write-Host "Packaged $package"
Write-Host "Zipped   $zip"
exit 0
