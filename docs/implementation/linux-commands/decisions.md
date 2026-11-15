# Implementation decisions

- Preserve the 46 inherited registry names. Phase 0 acceptance checks that the
  original 17 remain available; removing the already committed Phase 1 commands
  would violate the existing-command compatibility requirement.
- Apply the specific Phase 0 requirement in 05 §3 to Windows permission
  extraction: preserve existing output, record its conflict with 02 §7, and
  reconcile the shared synthesized modes during the authorized P2 ls/find work.
- Keep command-specific grammars, including find predicates and ps BSD options.
  Prefix expansion and POSIX stopping apply at each existing parser's option
  boundary; option values are never scanned independently for help or version.
- A skipped GNU case does not demonstrate option coverage. Executed CLI golden
  cases cover documented differences that cannot be compared with GNU.
- The maintainer requested the document name `linux-commands`; all current
  references use `docs/spec/linux-commands/`.
- Local Windows is available. No usable WSL or Docker installation was found.
  Linux GNU differential tests and macOS checks must run in GitHub Actions.
- The local inventory found no existing agentcmd installation on PATH or in the
  documented installation directories. Delivery will use the documented per-user
  install directory and verify the installed binaries explicitly.
