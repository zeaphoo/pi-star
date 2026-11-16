# Pi Star (pi-star)

**English** · [简体中文](README.md)

**An all-in-one coding desktop app built around a code agent.** Direct the agent, read code, review
diffs, and run your terminal without ever leaving the window.

pi-star brings the pieces of a coding loop into one cross-platform desktop app: while the agent edits
code, you open files, review the Git diff, run tests, and debug in the same window. It builds on the
session and tooling ecosystem of [pi coding agent](https://github.com/earendil-works/pi) and keeps
using the data under `~/.pi/agent`, so it shares sessions with the pi CLI and pi-web.

## Features

### 1. One app for the agent, the code, the diff, and the terminal

pi-star is not a set of tools placed side by side; it puts a complete coding loop in one window:

| Capability | What pi-star gives you |
| --- | --- |
| Directing the agent | Session tree and branches, queue/interrupt/retry, subagents, tool calls and thinking, compaction; switch model, thinking level, tools, and engine right from the composer |
| Code browser | Project tree, tabbed file viewer, syntax highlighting, `@` file index, and search; open a file the agent just changed and compare it on the spot |
| Git diff viewer | Staged and working-tree changes side by side, per-file diffs, inline highlighting, and line counts; review the agent's work and self-check before committing in one place |
| Terminal | Built-in PTY terminal for running, testing, and debugging, with configurable theme, font, size, and scrollback; shares the cwd and process environment with the agent |
| Trace view | Compresses an entire session into a filterable event timeline, so long sessions stay easy to follow |

No more switching between editor, terminal, browser, and git client, and no more copying the agent's
output somewhere else to read it. That is the essential difference between pi-star and an
"agent plus external editor" setup.

### 2. Two agent engines: native PI and Rust PI Agent

| Engine | Implementation | Notes |
| --- | --- | --- |
| **Native PI (Node.js)** | `pi --mode rpc` subprocess, same codebase as the pi CLI | Default; full compatibility with TypeScript extensions, MCP, subagents, and the pi ecosystem |
| **Rust PI Agent (QuickPI)** | Rust implementation of pi (merged from [pi-rust](https://github.com/bigfish1913/pi-rust)) running in-process | No Node required; fast startup and low overhead; built-in tools, MCP, subagents, and skill loading |

The engine is recorded per session in a custom entry, can be switched while the session is idle, and
is locked while it runs. Both engines share the same session files and configuration.

### 3. UI: learned from pi-web, redesigned for the desktop

The UI references and learns from [pi-web](https://github.com/agegr/pi-web) (MIT) but was redesigned
for desktop use: a flatter hierarchy and less visual noise, keeping the attention on code and
conversation.

## Download

Installers are published on [GitHub Releases](https://github.com/zeaphoo/pi-star/releases). Only the
desktop app is published; every package bundles Node, the pi CLI, and (on Windows) MinGit. You do not
need to install Node, pi, or Git first.

| Platform | Package |
| --- | --- |
| Linux | `pi-star-app_<version>_linux_<arch>.deb` (Debian/Ubuntu), `..._linux_<arch>.AppImage` |
| Windows | `pi-star-app_<version>_windows_x64-setup.exe` (NSIS, recommended), `..._windows_x64_<locale>.msi`, `..._windows_x64_portable.zip` |
| macOS | `pi-star-app_<version>_macos_<arch>.dmg` (Intel `x86_64` / Apple Silicon `arm64`) |

System requirements:

- **Linux**: x86_64 or arm64 with WebKitGTK 4.1 (available on Ubuntu 22.04+, Debian 12+, and
  similar; the deb pulls in its dependencies).
- **Windows**: Windows 10 / 11 (x64) with the WebView2 runtime (normally installed with Edge; the
  installer downloads it automatically when missing, or use the portable build).
- **macOS**: macOS 11+ on Intel or Apple Silicon, using the system WebKit.

## Install

### Linux

deb (recommended; handles dependencies and desktop integration):

```bash
sudo apt install ./pi-star-app_<version>_linux_amd64.deb
```

AppImage (no installation):

```bash
chmod +x pi-star-app_<version>_linux_amd64.AppImage
./pi-star-app_<version>_linux_amd64.AppImage
```

After installing, launch **PiStar** from the application menu; an AppImage just needs a
double-click.

### Windows

- **Installer**: run `pi-star-app_<version>_windows_x64-setup.exe` and follow the wizard (it installs
  to Program Files and needs administrator rights). The `.msi` is also available for managed or
  silent deployment.
- **Portable**: unzip `..._portable.zip` and keep `PiStarApp.exe`, `pi-star.exe`, and `pi-runtime/`
  in the same directory, then double-click `PiStarApp.exe`. No installation or admin rights needed.
- MinGit is bundled, so a separate Git for Windows installation is not required.
- The installers are currently unsigned, so SmartScreen may warn about an unknown publisher:
  choose **More info** → **Run anyway**.

### macOS

Open `pi-star-app_<version>_macos_<arch>.dmg` and drag **PiStar** into Applications.

The official build is signed with an Apple Developer certificate and notarized, so the first launch
needs no extra steps.

## First launch

- The desktop shell starts the bundled local service and loads the UI on a fixed
  `127.0.0.1:30151` (loopback only).
- To pick up existing work, open a project directory; sessions already in `~/.pi/agent` show up
  immediately.
- Closing the window keeps the app in the tray by default. The tray menu can quit, toggle launch at
  login, and toggle "close to tray".
- A background check looks at GitHub Releases and shows a link when a newer version exists; it never
  installs automatically. To upgrade, install the new package over the old one.
- Desktop preferences and logs live in `~/.local/share/net.quadhub.pi-star/` on Linux,
  `~/Library/Application Support/net.quadhub.pi-star/` on macOS, and
  `%APPDATA%\net.quadhub.pi-star` on Windows. Sessions and credentials stay in `~/.pi/agent`
  (override with `PI_CODING_AGENT_DIR`).

Uninstalling the app does not remove `~/.pi/agent` or the desktop preferences; delete those
directories manually for a full cleanup.

## More documentation

| Document | Contents |
| --- | --- |
| [`docs/development.md`](docs/development.md) | Architecture, building from source, development/testing, desktop packaging, security model (Chinese) |
| [`docs/status.md`](docs/status.md) | Implementation status and runtime boundaries (Chinese) |
| [`CHANGELOG.md`](CHANGELOG.md) | Release notes |
| [`packaging/README.md`](packaging/README.md) | Bundle contents, runtime versions, packaging details (Chinese) |

## License

pi-star is released under the **Apache License 2.0**; see [`LICENSE`](LICENSE). Code copied from
pi-web and pi-rust remains under MIT; upstream attributions, the full MIT text, and font licenses are
in [`NOTICE`](NOTICE).
