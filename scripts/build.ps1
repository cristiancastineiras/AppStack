param(
    [switch]$DebugBuild,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
& {
    Push-Location (Join-Path $PSScriptRoot '..')
    $previousRustFlags = $env:RUSTFLAGS
    try {
        # A static CRT keeps the executable free of the Visual C++ redistributable.
        $env:RUSTFLAGS = ($previousRustFlags + ' -C target-feature=+crt-static').Trim()
        $arguments = @('--locked','--message-format','short','--bin','AppStackClassic')
        if ($CheckOnly) { $operation = 'check' } else { $operation = 'build'; if (-not $DebugBuild) { $arguments += '--release' } }
        cargo $operation @arguments
        if ($LASTEXITCODE -ne 0) { throw 'AppStack compilation failed' }
    } finally { $env:RUSTFLAGS = $previousRustFlags; Pop-Location }
}
