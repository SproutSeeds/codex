# Issue 14015 Windows Handoff

This packet is a clean handoff for `openai/codex` issue `#14015`:

- issue: <https://github.com/openai/codex/issues/14015>
- branch intent: preserve the current Windows investigation state so it can be pulled onto a Windows machine directly
- current status: reproduced and narrowed, not patched

## What We Proved

On the same Windows host with official `codex-cli 0.115.0`:

- from an SSH / network-logon Codex session in session `0`, sandboxed `powershell.exe` fails with exit `-1073741502`
- from that same context, sandboxed `dotnet.exe --info` fails with CoreCLR load / bind errors
- sandboxed `cmd.exe /c echo ok` still succeeds there
- sandboxed `reg.exe query HKCU\\Environment` still succeeds there
- inside a successfully sandboxed `cmd.exe`, `powershell.exe`, `dotnet.exe`, and `whoami /groups` still fail from that same SSH / network-logon session
- on the same host, sandboxed PowerShell and sandboxed `dotnet.exe` both succeed from interactive scheduled tasks at both medium and high integrity

The strongest boundary we have is:

- legacy restricted-token backend + SSH / network-logon token: fails
- same machine + interactive token: succeeds

## What This Means

This does not currently look like a PowerShell expression problem.

It also does not look like a shell-wrapper-only problem, because the same failures persist for children of a successfully sandboxed `cmd.exe`.

The likely fix surface is in the legacy Windows restricted-token backend, not in `shell.rs`.

## Strongest Patch Direction

The current best patch direction is:

1. Detect non-interactive / network-logon base tokens in the legacy restricted-token path.
2. Fall back to the elevated sandbox-user runner when it is available.
3. If that fallback is not available, fail with a targeted error instead of silently returning empty output.

This is a patch direction, not a validated fix.

## Code Pointers

- `codex-rs/windows-sandbox-rs/src/lib.rs`
- `codex-rs/windows-sandbox-rs/src/process.rs`
- `codex-rs/windows-sandbox-rs/src/token.rs`
- `codex-rs/windows-sandbox-rs/src/elevated_impl.rs`
- `codex-rs/core/src/exec.rs`
- `codex-rs/core/src/shell.rs`

## Recommended Local Checks

Run the probe script from a local Windows console first:

```bat
set CODEX_BIN=C:\path\to\codex.exe
docs\investigations\issue-14015-windows\network_logon_probe.cmd
```

Then compare that with:

- the same script from your SSH / remote context
- the interactive TUI repro for `echo $env:PATH`
- the interactive TUI repro for `cmd.exe /c echo %PATH%`

If the local console succeeds while SSH fails, that confirms the current boundary is logon-context-specific.

If the local console also fails, then the public issue comment can be broadened beyond the SSH / network-logon scope.

## Current Draft Comment

See [draft-comment.md](./draft-comment.md).
