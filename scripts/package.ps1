param([switch]$SkipChecks)
$ErrorActionPreference = 'Stop'
& {
    Push-Location (Join-Path $PSScriptRoot '..')
    try {
        & '.\scripts\build.ps1'
        if (-not $SkipChecks) {
            cargo fmt --all -- --check
            if ($LASTEXITCODE -ne 0) { throw 'Rust formatting check failed' }
            cargo clippy --all-targets --locked -- -D warnings
            if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
            cargo test --all-targets --locked
            if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
        }
        $version = ((Get-Content 'Cargo.toml' | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value)
        if (-not $version) { throw 'Package version was not found' }
        # Release asset names stay the same across versions; the updater looks them up by name.
        $binary = 'AppStackClassic'
        $executable = Join-Path 'target\release' ($binary + '.exe')
        New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
        # The "portable" in its name makes this copy keep its data in a config folder beside it.
        $portable = Join-Path 'dist' ($binary + '-portable.exe')
        Copy-Item $executable $portable -Force
        Get-FileHash $portable -Algorithm SHA256
        Write-Output ('Portable executable: ' + $portable)
        $directory = Join-Path 'dist' 'installer-files'
        Remove-Item $directory -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        Copy-Item $executable,'README.md','LICENSE','THIRD-PARTY-NOTICES.md','fonts\Inter-OFL.txt','fonts\SpaceGrotesk-OFL.txt' $directory -Force
        $nsis = Get-Command makensis -ErrorAction SilentlyContinue
        if ($nsis) {
            $icon = Get-ChildItem 'target\release\build\appstack-*\out' -Filter ($binary + '.ico') -Recurse |
                Sort-Object LastWriteTime -Descending | Select-Object -First 1
            $setup = Join-Path $PWD (Join-Path 'dist' ($binary + '-setup.exe'))
            & $nsis.Source /V2 "/DNAME=$binary" "/DEXE=$binary.exe" "/DVERSION=$version" "/DSOURCE=$(Join-Path $PWD $directory)" "/DICON=$($icon.FullName)" "/DOUTFILE=$setup" 'installer\appstack.nsi'
            if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }
            Get-FileHash $setup -Algorithm SHA256
            Write-Output ('Installer: ' + $setup)
        } else { Write-Warning 'makensis (NSIS) was not found; skipping the installer' }
    } finally { Pop-Location }
}
