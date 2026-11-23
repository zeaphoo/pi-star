# Desktop packaging

**English** · [简体中文](README.zh-CN.md)

Pi Star only ships the **desktop app**; there is no separate Linux server package anymore. Every
installer contains:

| Content | Location |
| --- | --- |
| Desktop shell | `PiStarApp` / `PiStarApp.exe` / `PiStar.app` |
| pi-star server | `pi-star` under the resources directory (same directory as the shell on Windows) |
| Node runtime | `pi-runtime/node` (`node.exe` on Windows) |
| pi CLI | `pi-runtime/pi/` (`@earendil-works/pi-coding-agent` + production dependencies) |
| pi-star extension | `pi-runtime/agent-extension/` (`index.js`, `subagent.js`, `package.json`) |
| MinGit (Windows only) | `pi-runtime/git/` (Git Bash, git, coreutils) |

Linux installs to `/usr/lib/PiStar/`, macOS to `Contents/Resources/`, and Windows to the install
directory. `pi-star` looks for `pi-runtime/` next to itself, so no separate Node or pi installation is
needed after installing.

## Versions

`packaging/pi-runtime.json` pins the bundled runtime:

```json
{
  "node": "22.20.0",
  "pi": "1.6.0",
  "minGit": "2.56.0.4",
  "minGitTag": "v2.56.0.windows.4"
}
```

Update this file and re-package when upgrading pi/Node/MinGit. Windows targets do not need Git for
Windows preinstalled; `pi-star` uses `pi-runtime/git/bin/bash.exe` and adds `git`/`cmd`/`usr/bin` and
similar directories to the child process PATH. `scripts/check-desktop-version.sh` verifies
`app/Cargo.toml`, `app/tauri.conf.json`, and the workspace version; release tags must be
`v<version>`.

## Local builds

```bash
./scripts/check-desktop-version.sh
./scripts/package-desktop.sh              # current platform
# or individually:
./scripts/package-linux-app.sh            # deb + AppImage
./scripts/package-windows.sh              # NSIS + MSI + portable zip (Git Bash)
./scripts/package-mac.sh                  # .app + dmg
```

Common environment variables:

- `SKIP_UI_BUILD=1`: reuse an existing `ui/dist`.
- `SKIP_PI_RUNTIME=1`: reuse `app/pi-runtime` (must match the current platform/architecture).
- `PI_RUNTIME_DIR=...`: custom runtime staging directory.
- `TAURI_BUNDLES=deb` / `nsis`: build only some installers.
- `DIST_DIR=...`: custom output directory.
- `MAC_SIGN_IDENTITY=...`: macOS signing identity.

The runtime preparation script can also be run on its own:

```bash
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform linux --arch x64
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform win32 --arch x64
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform darwin --arch arm64
```

It downloads the pinned Node version from nodejs.org, runs `npm pack` for the pinned pi version,
installs production dependencies with `npm install --omit=dev`, and copies `agent-extension/`; on
Windows it also extracts MinGit. It skips automatically when the staging directory is already
prepared with matching metadata. MinGit is GPL-2.0; installers include
`GIT-LICENSE.txt`/`GIT-NOTICE.txt` and the source URL <https://github.com/git-for-windows/git>.
Installers also carry the project `LICENSE` (Apache-2.0) and `NOTICE` (with the pi-web/pi-rust MIT
attributions and font licenses) in the resources directory (Linux `/usr/lib/PiStar/`, macOS
`Contents/Resources/`, Windows next to the main executable).

## Artifact names

| Platform | Artifacts |
| --- | --- |
| Linux | `pi-star-app_<ver>_linux_<arch>.deb`, `.AppImage` |
| Windows | `pi-star-app_<ver>_windows_<arch>-setup.exe`, `..._<locale>.msi`, `..._portable.zip` |
| macOS | `pi-star-app_<ver>_macos_<arch>.dmg` |

The Windows main executable is `PiStarApp.exe`, which remains distinct from `pi-star.exe` even
case-insensitively. The portable zip keeps both executables and `pi-runtime/` in the same directory.
`pi/node_modules` paths are deep, so Windows build machines should enable long paths
(`git config --global core.longpaths true` and the registry `LongPathsEnabled=1`); CI enables this
automatically. `PI_RUNTIME_DIR` on Windows must be on the same drive as the repository: Tauri strips
the drive prefix when normalizing resource paths, and cross-drive paths resolve to the wrong location
at build time. The packaging scripts validate this early and fail with an error.

## CI / Release

`.github/workflows/ci.yml` runs the Rust workspace + desktop shell tests and UI tests/build on Linux,
and desktop shell compile checks on Linux/Windows/macOS.

`.github/workflows/release.yml` is triggered by `v*` tags (or manually):

1. verify the tag, the version manifest, and that `## [<version>]` exists in `CHANGELOG.md`;
2. Linux produces deb + AppImage;
3. Windows produces NSIS, MSI, and portable zip;
4. macOS produces dmg on Intel/Apple Silicon runners;
5. `scripts/release-notes.sh` extracts the CHANGELOG section as the body, aggregates the artifacts,
   and creates the GitHub Release (a manual run with `publish=false` only uploads artifacts).

Update `CHANGELOG.md` before releasing:

```markdown
## [0.1.0] - 2026-10-31
### Added
- ...
```

### macOS signing and notarization

Official Release macOS builds are signed, notarized, and stapled, so users need no extra steps on
first launch. Local builds without the secrets below produce an **unsigned** dmg. Signing and
notarization are driven by these secrets:

| secret | Purpose |
| --- | --- |
| `MAC_CERT_P12_BASE64` | Developer ID Application certificate + full chain |
| `MAC_CERT_PASSWORD` | p12 password |
| `MAC_SIGN_IDENTITY` | Full signing identity name |
| `MAC_NOTARY_API_KEY_ID` | App Store Connect API key id |
| `MAC_NOTARY_API_ISSUER_ID` | issuer id |
| `MAC_NOTARY_API_KEY_P8_BASE64` | .p8 private key (base64) |

Signing adds JIT/unsigned-memory entitlements to the bundled Node (see
`packaging/macos/entitlements.plist`), signs every Mach-O under `pi-runtime`, then signs the outer
`.app`; the notarization script notarizes the app, staples the ticket, rebuilds the dmg, and
notarizes the dmg. `stapler` may hit transient network errors when fetching the ticket from Apple
CloudKit (Error 65/68, NSURLError -1005); the script retries 4 times with a 20 second delay by
default, adjustable with `STAPLER_RETRIES` / `STAPLER_RETRY_DELAY`.

## Windows bash

On Windows, the agent's `bash` tool prefers the MinGit bundled in the installer; it never falls back
to `cmd.exe` and never uses WSL's `System32\bash.exe`. Probe order:

1. bundled `pi-runtime/git/bin/bash.exe` (or `PI_STAR_GIT_HOME`);
2. `PI_STAR_BASH`, also accepting `PI_DESKTOP_BASH`;
3. derive `Git\bin\bash.exe` / `Git\usr\bin\bash.exe` from `git.exe` on PATH;
4. `Git\bin\bash.exe` under `ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`,
   `LocalAppData\Programs`;
5. `where bash.exe`, excluding the WSL launcher in `System32`.

Once found it runs `bash --noprofile --norc -c` with `CREATE_NO_WINDOW`. The terminal panel still
defaults to PowerShell (override with `SHELL`).
