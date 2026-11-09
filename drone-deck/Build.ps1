$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --manifest-path Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'Compilation failed' }
    $droneDestination = Split-Path $PSScriptRoot -Parent
    Copy-Item -LiteralPath 'target\release\drone-deck.exe' -Destination (Join-Path $droneDestination 'Drone Deck.exe') -Force
    $droneAssets = Join-Path $droneDestination 'assets'
    New-Item -ItemType Directory -Path $droneAssets -Force | Out-Null
    Get-ChildItem -LiteralPath 'assets' -File | Copy-Item -Destination $droneAssets -Force
    $droneScripts = Join-Path $droneDestination 'runtime'
    New-Item -ItemType Directory -Path $droneScripts -Force | Out-Null
    Get-ChildItem -LiteralPath 'scripts' | Copy-Item -Destination $droneScripts -Recurse -Force
    Write-Host 'Built Drone Deck.exe in the project root.'
} finally { Pop-Location }
