SolveCraft is open-source parametric 3D CAD in Rust: sketch, constrain, extrude, and change a
parameter to watch the timeline rebuild. [Documentation](https://bherbruck.github.io/solvecraft/)
· [try it in the browser](https://bherbruck.github.io/solvecraft/app/) ·
[contributing](https://github.com/bherbruck/solvecraft/blob/main/CONTRIBUTING.md) ·
[issues](https://github.com/bherbruck/solvecraft/issues). AI agents can model with it over MCP:
`solvecraft-cli mcp` ([how](https://bherbruck.github.io/solvecraft/guide/mcp.html)).

**Which file?**
- **Windows:** `SolveCraft-{version}-windows-x64.exe` runs as is. Or install the `.msi`, or unzip the
  `-portable.zip` (x64, x86 and arm64).
- **macOS 11+** (Apple silicon and Intel): open the `.dmg` and drag SolveCraft onto Applications.
  The CLI is in `solvecraft-cli-{version}-macos-universal.zip`.
- **Linux** (x86_64 and aarch64, glibc 2.35+): the `.AppImage` (`chmod +x`, then run it), the
  `.deb` or `.rpm`, the `.flatpak` (`flatpak install --user <file>`), or the `.tar.gz`.
- **FreeBSD:** `tar -xzf solvecraft-{version}-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local`.
- **Web:** `solvecraft-web-{version}.zip` is a static site to host yourself.
- `SHA256SUMS.txt`: `sha256sum -c SHA256SUMS.txt --ignore-missing`.

The builds aren't signed yet. On macOS, right-click SolveCraft in Applications and choose
**Open** the first time (or run `xattr -dr com.apple.quarantine /Applications/SolveCraft.app`).
On Windows, SmartScreen may say "Windows protected your PC": **More info**, then **Run anyway**.
Licences are built in (Help ▸ About ▸ Licences, `solvecraft-cli licences`); the packages also
install `THIRD-PARTY-LICENSES.txt`. Known limitations:
[ROADMAP.md](https://github.com/bherbruck/solvecraft/blob/main/ROADMAP.md#known-limitations).

