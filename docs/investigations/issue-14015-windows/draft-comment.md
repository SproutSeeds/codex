I verified the candidate fix on native Windows from the same SSH / network-logon path that previously produced empty output and `-1073741502`.

- Umbra currently has no sandbox setup marker or sandbox users file under `C:\Users\codyr\.codex`
- with a GitHub-built Windows binary from commit `ce5781c`, every probe command in that SSH / session `0` path now returns the targeted `Windows restricted-token sandbox cannot launch reliably from this non-interactive or network-logon session until elevated Windows sandbox setup has completed...` error with `EXIT=1`
- that includes direct `powershell.exe`, direct `dotnet.exe --info`, direct `cmd.exe /c echo ok`, direct `reg.exe query HKCU\Environment`, and the child `powershell.exe` / `dotnet.exe` / `whoami /groups` commands through sandboxed `cmd.exe`
- `C:\Users\codyr\.codex\.sandbox\sandbox.log` shows `legacy sandbox: session_id=0 interactive=false remote_interactive=false network=true -> elevated setup required`

The old SSH / network-logon failure mode is no longer reproducing on this host.
The guard is firing on the right path and surfacing the setup-missing state directly.
This verifies the fallback-or-targeted-error fix shape for the non-interactive / network-logon case.
