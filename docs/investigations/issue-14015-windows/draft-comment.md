I reproduced a narrower boundary on native Windows with official `codex-cli 0.115.0`.

- from an SSH / network-logon Codex session in session `0`, `codex sandbox windows powershell.exe -NoProfile -Command "Write-Output ok"` exits `-1073741502` with no stdout or stderr
- in that same context, `codex sandbox windows dotnet.exe --info` fails with CoreCLR load / bind errors
- `codex sandbox windows cmd.exe /c echo ok` and `codex sandbox windows reg.exe query HKCU\Environment` still succeed there
- the failure is not limited to the initial launcher: inside a successfully sandboxed `cmd.exe`, `powershell.exe -NoProfile`, `dotnet.exe --info`, and `whoami /groups` still fail from the same SSH / network-logon session
- on the same host, the sandboxed PowerShell and `dotnet.exe` commands both succeed from scheduled tasks running under interactive tokens at both medium and high integrity
- the TUI `shell_command` path can still hide `cmd.exe /c echo %PATH%` behind the same PowerShell failure because that wrapper launches the command string through PowerShell first

This is a legacy Windows restricted-token backend problem for SSH / network-logon sessions, not a PowerShell expression syntax problem.
The fix surface is below `shell.rs`, and below the initial `CreateProcessAsUserW` command line, because the same failures persist for children of a successfully sandboxed `cmd.exe`.
In `windows-sandbox-rs`, the legacy path builds a restricted token from the current logon token and launches it directly, while the elevated path uses `CreateProcessWithLogonW(LOGON_WITH_PROFILE)` for a sandbox user runner.
Detect non-interactive / network-logon base tokens in the legacy path and fall back to the elevated sandbox-user runner when available, or fail with a targeted error instead of silently returning empty output.
