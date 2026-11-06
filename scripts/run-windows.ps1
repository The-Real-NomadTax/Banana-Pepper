param(
    [string]$GamesRoot = 'games',
    [ValidateSet('workbench', 'probe', 'launch', 'setup')]
    [string]$Mode = 'workbench',
    [ValidateSet('minecraft','big-buck','ford')]
    [string]$Game = 'minecraft',
    [ValidateSet('auto','water','native','wsl')]
    [string]$Backend = 'auto'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    if ($Mode -ne 'workbench' -and $Game -eq 'ford') {
        if ($Mode -ne 'launch') { throw 'Ford supports -Mode launch.' }
        & cargo build -p banana-ford-hook --target i686-pc-windows-msvc --release
        if ($LASTEXITCODE -ne 0) { throw 'Ford adapter build failed. Install rustup target i686-pc-windows-msvc and LLVM lld-link.' }
    }
    if ($Mode -ne 'workbench' -and $Game -eq 'big-buck' -and $Backend -ne 'wsl') {
        & cargo build -p banana-water --bin water-buck --target i686-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw 'Water Big Buck worker build failed.' }
    }
    & cargo build -p banana-desktop
    if ($LASTEXITCODE -ne 0) { throw 'Banana pepper build failed.' }
    $action = if ($Mode -ne 'workbench' -and $Game -eq 'ford') { '--launch-ford' }
    elseif ($Mode -ne 'workbench' -and $Game -eq 'big-buck') {
        if ($Mode -eq 'setup') { throw 'See docs/big-buck-windows.md for WSL library setup.' }
        if ($Mode -eq 'probe') { '--probe-big-buck' } else { '--launch-big-buck' }
    } else { switch ($Mode) {
        'probe' { '--probe-minecraft' }
        'launch' { '--launch-minecraft' }
        'setup' { '--setup-minecraft-libs' }
        default { '--games-root' }
    } }
    $previousBackend = $env:ARCADE_BBH_BACKEND
    try {
    if ($Game -eq 'big-buck') { $env:ARCADE_BBH_BACKEND = $Backend }
    & (Join-Path $projectRoot 'target\debug\arcade-desktop.exe') $action $GamesRoot
    if ($LASTEXITCODE -ne 0) { throw 'Banana pepper reported a runtime failure. See the diagnostic output above.' }
    } finally { $env:ARCADE_BBH_BACKEND = $previousBackend }
}
finally { Pop-Location }
