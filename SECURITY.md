# Security Policy

## Supported versions

Only the latest release receives security fixes. AppStack checks for updates
by default and installs them when you accept, so staying current is usually one
click.

## Reporting a vulnerability

Please do not open a public issue for security problems. Report them privately
through GitHub instead:

1. Go to the repository's **Security** tab.
2. Choose **Report a vulnerability**
   (<https://github.com/cristiancastineiras/AppStack/security/advisories/new>).
3. Describe the problem, the affected version and how to reproduce it.

The maintainer aims to reply within a week. Once a fix is released, the advisory
is published and you are credited unless you prefer otherwise.

## Scope

Useful reports include, for example:

- The updater installing something that did not come from this repository's
  releases.
- A crafted `appstack.json`, `.agz` backup or TaskbarGroups import that makes
  AppStack run code, write outside its data folder or overwrite unrelated files.
- AppStack starting programs, or starting them elevated, without the user asking.

AppStack starts the programs you put in your groups; launching a program you
added yourself is expected behaviour, not a vulnerability.

## How updates are protected

The updater only talks to GitHub over HTTPS, only downloads assets from this
repository's releases, and checks that the downloaded size matches what GitHub
reports. The executables are not code-signed yet, so Windows SmartScreen may
warn the first time you run a new download.

Community styles are downloaded from this repository the same way. They are plain
popup settings, validated like your own before use (sizes in range, colors as
`#RRGGBB`, short names); they cannot run anything or reference files.
