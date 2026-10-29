# Building installers with IronSetup

Windows: build the release binaries with `cargo build --release --bins --locked` and place the 17 command `.exe` files in `dist/windows-x86_64/`. Obtain IronSetup 0.1.0 separately and verify its distribution checksums. Then run its Windows compiler:

```powershell
ironsetup.exe check packaging/windows.iss
ironsetup.exe compile packaging/windows.iss --stub path/to/ironsetup-stub.exe --output dist/Agent-Commands-0.1.0-Windows-Setup.exe
```

The installer creates an optional terminal shortcut; it does not change the persistent PATH. Windows and Linux `.iss` source files and quick-start text are included here. The Linux script expects its `payload/` directory to contain the Linux x86-64 release binaries, README, LICENSE, QuickStart.txt and shell/cd.bash; compile it on Linux so executable permissions are recorded correctly. Linux needs glibc 2.39 or newer. IronSetup 0.1.0 does not provide a macOS installer stub.
