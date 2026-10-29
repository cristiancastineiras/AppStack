# Third-Party Notices

AppStack is MIT licensed. Dependencies are governed by their own licenses; the
project license does not replace those terms.

The executable embeds the Inter (Regular) and Space Grotesk (SemiBold) fonts,
licensed under the SIL Open Font License 1.1. Their license texts are in `fonts/`,
are copied into the installation, and come with every release in its source
archives (the portable executable is a single file). The fonts are
registered privately for the running process and are not installed on the system.

Button and menu icons are Lucide icons (lucide-static 1.64.0, <https://lucide.dev>),
ISC licensed, with some derived from Feather under the MIT license. The SVG sources
are in `icons/` with their license text in `icons/Lucide-LICENSE.txt`, which is also
copied into the installation; the build renders them into the executable.

Other direct dependencies include windows-rs (MIT/Apache-2.0), serde and
serde_json (MIT/Apache-2.0), uuid (MIT/Apache-2.0), image (MIT/Apache-2.0),
zip (MIT), quick-xml (MIT), resvg (MPL-2.0) and ico (MIT).
Review Cargo.lock and dependency license files before publishing a release.
