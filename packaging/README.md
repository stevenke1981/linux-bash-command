# Building and staging packages

## Local Windows portable archive

Build the release binaries first, or supply an existing release build from the
same checkout. The packaging script does not build, install, publish, change
PATH, or remove existing files. It checks the binary version against
`Cargo.toml`, obtains the command list from that binary, and requires an empty
staging directory. Specify separate staging and output directories:

```powershell
cargo build --release --bins --locked
.\scripts\package.ps1 `
  -BinaryDirectory .\target\release `
  -StageDirectory .\dist\windows-x86_64 `
  -OutputDirectory .\dist\portable
```

For version 0.2.0 this creates
`dist/portable/agent-commands-0.2.0-windows-x86_64.zip` and `SHA256SUMS.txt`.
The script refuses to overwrite either output or use a non-empty stage. Choose
new directories for another run. It includes `agentcmd.exe`, every registered
standalone utility except Windows `link.exe` (which conflicts with the MSVC
linker), and `[.exe` copied from `test.exe`, along with README, LICENSE,
NOTICE, quick start, terminal launcher and shell integrations.

Extract the ZIP to a folder and open `Agent Commands.cmd` to start Command
Prompt with the tools on PATH for that session.

## Windows installer (Inno Setup)

The installer uses [Inno Setup 6](https://jrsoftware.org/isinfo.php). After
staging as above, run from the repository root:

```powershell
iscc /DAppVersion=0.2.0 packaging\agent-commands.iss
```

The output is `dist/Agent-Commands-0.2.0-Windows-Setup.exe`. The installer is
per user and adds the `Agent Commands Terminal` shortcut to the Start menu.
The optional user PATH task makes the commands available in new terminals.
Uninstalling removes that PATH entry. A silent install may use
`Setup.exe /VERYSILENT /TASKS=addtopath`. Installing and changing PATH are
separate from the local staging script.

## Release workflow and legacy installers

`.github/workflows/release.yml` builds registry-derived archives for Windows,
Linux and macOS, an Inno Setup installer, and `SHA256SUMS.txt`. The Windows
release uses the same packaging script and ZIP contents as the local build.
To build without publishing, manually run the workflow with `tag` set to a
commit, branch or tag and leave `publish` unchecked (its default). Download the
combined `release-assets` artifact from the run. The workflow resolves the
supplied ref once and builds all platforms from that commit.

Pushing a version tag continues to publish a GitHub release. A manual run
publishes only when `publish` is explicitly checked; in either publication mode,
the tag must equal `v` followed by the package version in `Cargo.toml`. Local
packaging does not publish. Linux and macOS archives include the `[` alias and
Windows archives omit `link.exe`.

`windows.iss` and `linux.iss` are legacy IronSetup scripts. For the Linux
script, prepare `packaging/payload/` with `agentcmd`, every binary listed by
`agentcmd --list` (including `link` and the `[` alias), README.md, LICENSE,
NOTICE.md, `linux-quickstart.txt` copied to `QuickStart.txt`, and `shell/`.
Compile it on Linux so executable permissions are recorded correctly. Linux
release archives built by CI on Ubuntu 22.04 require glibc 2.35 or newer.
