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
        $binary = 'AppStackClassic'
        $name = $binary + '-' + $version + '-windows-portable'
        $directory = Join-Path 'dist' $name
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        # Removing first keeps the executable's casing; Windows keeps an old name on overwrite.
        Remove-Item (Join-Path $directory ($binary + '.exe')) -ErrorAction SilentlyContinue
        Copy-Item (Join-Path 'target\release' ($binary + '.exe')) (Join-Path $directory ($binary + '.exe')) -Force
        Copy-Item 'README.md','LICENSE','THIRD-PARTY-NOTICES.md','fonts\Inter-OFL.txt','fonts\SpaceGrotesk-OFL.txt' $directory -Force
        [IO.File]::WriteAllText((Join-Path (Join-Path $PWD $directory) 'portable.txt'),'')
        $archive = Join-Path 'dist' ($name + '.zip')
        # Top-level files only: a portable `config` folder left by running the package holds user data.
        # Antivirus scanners briefly lock the files just copied above.
        for ($attempt = 1; ; $attempt++) {
            try { Get-ChildItem -Path $directory -File | Compress-Archive -DestinationPath $archive -Force; break }
            catch { if ($attempt -ge 5) { throw }; Start-Sleep -Seconds 1 }
        }
        Get-FileHash $archive -Algorithm SHA256
        Write-Output ('Portable executable: ' + (Join-Path $directory ($binary + '.exe')))
        $nsis = Get-Command makensis -ErrorAction SilentlyContinue
        if ($nsis) {
            $icon = Get-ChildItem 'target\release\build\appstack-*\out' -Filter ($binary + '.ico') -Recurse |
                Sort-Object LastWriteTime -Descending | Select-Object -First 1
            $setup = Join-Path $PWD (Join-Path 'dist' ($binary + '-' + $version + '-setup.exe'))
            & $nsis.Source /V2 "/DNAME=$binary" "/DEXE=$binary.exe" "/DVERSION=$version" "/DSOURCE=$(Join-Path $PWD $directory)" "/DICON=$($icon.FullName)" "/DOUTFILE=$setup" 'installer\appstack.nsi'
            if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }
            Get-FileHash $setup -Algorithm SHA256
            Write-Output ('Installer: ' + $setup)
        } else { Write-Warning 'makensis (NSIS) was not found; skipping the installer' }
    } finally { Pop-Location }
}
