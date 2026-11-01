# Building installers

## Windows (Inno Setup)

The Windows installer is built with [Inno Setup 6](https://jrsoftware.org/isinfo.php) from `packaging/agent-commands.iss`.

GitHub releases are produced automatically by `.github/workflows/release.yml`: pushing a tag `vX.Y.Z` that matches the `Cargo.toml` version builds the commands on Windows, Linux and macOS, compiles the Inno Setup installer, smoke-tests a silent install, and publishes the installer, per-platform archives and `SHA256SUMS.txt` to a GitHub release.

To build the installer locally, stage every command `.exe` (as listed by `agentcmd --list`, plus `agentcmd.exe` and `[.exe` copied from `test.exe`) in `dist/windows-x86_64/`, then run from the repository root:

```powershell
cargo build --release --bins --locked
iscc /DAppVersion=0.1.0 packaging\agent-commands.iss
```

The installer is written to `dist/Agent-Commands-<version>-Windows-Setup.exe`. It installs per user (no administrator rights) to `%LOCALAPPDATA%\Programs\Agent Commands`, creates an Agent Commands Terminal shortcut, and offers an optional task that adds the install directory to the user PATH; uninstalling removes that PATH entry. Silent install: `Setup.exe /VERYSILENT /TASKS=addtopath`.

## Legacy IronSetup scripts

`windows.iss` and `linux.iss` are the earlier IronSetup 0.1.0 scripts. The Linux script expects its `payload/` directory to contain the Linux x86-64 release binaries, README, LICENSE, `linux-quickstart.txt` and `shell/cd.bash`; compile it on Linux so executable permissions are recorded correctly. Release archives built by CI on Ubuntu 22.04 need glibc 2.35 or newer.
