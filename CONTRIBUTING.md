# Contributing to AppStack

Thanks for helping. Bug reports, ideas and pull requests are all welcome. By
taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting bugs and asking for features

Open an issue with the **Bug report** or **Feature request** form. For bugs,
include your Windows version, the AppStack version (**Settings > About AppStack**),
whether you use the installer or the portable executable, and the steps that
reproduce the problem. `errors.log` in the data folder (**File > Open data
folder**) often helps.

Security problems are different: please report them privately as described in
[SECURITY.md](SECURITY.md).

## Development setup

You need Windows 10/11 x64, Rust 1.85 or newer with the MSVC toolchain, and
Visual Studio Build Tools with the Desktop development with C++ workload. NSIS
(`makensis`) is only needed to build the installer.

```powershell
.\scripts\build.ps1 -DebugBuild
.\target\debug\AppStackClassic.exe --data-dir "$env:TEMP\appstack-dev"
```

`--data-dir` keeps your test groups away from your real ones.

## Before opening a pull request

Run the same checks as CI:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

For changes to popups or the editor, also run the UI smoke tests described in
the README's **Verification** section and look at the screenshots they write.

Keep changes focused: one fix or feature per pull request, written like the
code around it (plain Win32 through the `windows` crate; no new runtime or UI
framework). Mention any user-visible change in the README.

## Commit messages and releases

Commits follow [Conventional Commits](https://www.conventionalcommits.org/),
because releases and the changelog are generated from them:

| Prefix | Use for | Version change |
| --- | --- | --- |
| `feat:` | A new feature | minor (1.**1**.0) |
| `fix:` | A bug fix | patch (1.0.**1**) |
| `perf:` | A performance improvement | patch |
| `docs:` | Documentation only | none (listed in the changelog) |
| `refactor:`, `test:`, `build:`, `ci:`, `chore:` | Everything else | none (hidden) |

Add `!` after the type (`feat!:`) or a `BREAKING CHANGE:` footer for changes
that break existing configurations or commands; that raises the major version.
An optional scope names the area: `fix(popup): keep the pill shape on high DPI`.

Pull requests should be squash-merged, so the pull request title is what lands
on `main`; give it the same format.

### How a release happens

1. Every push to `main` runs CI and updates an open **release pull request**
   that bumps the version in `Cargo.toml` and adds the new section to
   `CHANGELOG.md`.
2. Merging that pull request tags the version (`vX.Y.Z`) and publishes the
   GitHub release with the changelog as its notes.
3. The release workflow then builds and attaches `AppStackClassic-setup.exe` and
   `AppStackClassic-portable.exe`. Running copies of AppStack find the new
   release within a day (or at once with **Check for updates**) and offer to
   install it.

Do not edit the version or the changelog by hand; the release pull request
does both.
