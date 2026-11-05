param(
    [string]$GamesRoot = 'games',
    [ValidateSet('workbench', 'probe', 'launch', 'setup')]
    [string]$Mode = 'workbench'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    & cargo build -p arcade-desktop
    if ($LASTEXITCODE -ne 0) { throw 'Arcade Foundry build failed.' }
    $action = switch ($Mode) {
        'probe' { '--probe-minecraft' }
        'launch' { '--launch-minecraft' }
        'setup' { '--setup-minecraft-libs' }
        default { '--games-root' }
    }
    & (Join-Path $projectRoot 'target\debug\arcade-desktop.exe') $action $GamesRoot
    if ($LASTEXITCODE -ne 0) { throw 'Arcade Foundry reported a runtime failure. See the diagnostic output above.' }
}
finally { Pop-Location }
