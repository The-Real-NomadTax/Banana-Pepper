param([string]$GamesRoot = 'games', [string]$Game = '', [string]$Action = 'launch')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    & just build
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    $executable = Join-Path $projectRoot 'target/dist/banana-pepper-windows-x86_64/arcade-desktop.exe'
    if ($Game) { & $executable --action $Game $Action $GamesRoot }
    else { & $executable --games-root $GamesRoot }
    if ($LASTEXITCODE -ne 0) { throw 'Banana Pepper reported a session failure.' }
} finally { Pop-Location }
