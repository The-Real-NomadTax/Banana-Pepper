# Development build helper only. No shell interpreter is used in guest launches.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $wslOptions = @()
    if ($env:ARCADE_WSL_DISTRO) { $wslOptions = @('--distribution', $env:ARCADE_WSL_DISTRO) }
    $artifactMarker = Join-Path $projectRoot 'target\wsl\package-ready'
    if (Test-Path -LiteralPath $artifactMarker) { Remove-Item -LiteralPath $artifactMarker }
    $linuxRoot = (& wsl.exe @wslOptions --exec wslpath -a -u $projectRoot).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot access WSL. Install a Linux distro with a Rust/C++ toolchain.' }
    function Invoke-WslRust([string]$Program, [string[]]$Arguments) {
        $localToolchain = Join-Path $projectRoot 'runtime\wsl-toolchain\cargo\bin'
        if (Test-Path -LiteralPath (Join-Path $localToolchain $Program)) {
            & wsl.exe @wslOptions --cd $linuxRoot --exec env "CARGO_HOME=$linuxRoot/runtime/wsl-toolchain/cargo" "RUSTUP_HOME=$linuxRoot/runtime/wsl-toolchain/rustup" "$linuxRoot/runtime/wsl-toolchain/cargo/bin/$Program" @Arguments
        } else {
            & wsl.exe @wslOptions --cd $linuxRoot --exec $Program @Arguments
        }
        if ($LASTEXITCODE -ne 0) { throw "WSL $Program failed ($LASTEXITCODE)." }
    }
    Invoke-WslRust 'cargo' @('build', '-p', 'banana-compat', '--bin', 'banana-game-host', '-p', 'banana-runtime', '--bin', 'banana-session', '--release', '--locked', '--target-dir', 'target/wsl')
    Invoke-WslRust 'cargo' @('build', '-p', 'banana-minecraft-io', '--release', '--locked', '--target-dir', 'target/wsl')
    Invoke-WslRust 'rustup' @('target', 'add', 'i686-unknown-linux-gnu')
    Invoke-WslRust 'cargo' @('build', '-p', 'banana-linux-shim', '--target', 'i686-unknown-linux-gnu', '--release', '--locked', '--target-dir', 'target/wsl')
    [System.IO.File]::WriteAllText($artifactMarker, "Compiled WSL bundle v1`n")
} finally { Pop-Location }
