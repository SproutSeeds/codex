# Issue 14015 Windows Handoff

This packet is a clean handoff for `openai/codex` issue `#14015`:

- issue: <https://github.com/openai/codex/issues/14015>
- branch intent: preserve the current Windows investigation state so it can be pulled onto a Windows machine directly
- current status: reproduced and narrowed; the candidate patch is now verified on Umbra's SSH / network-logon path for the setup-missing case

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

## Current Branch Patch

This branch now contains a candidate patch that implements the previously
identified patch direction:

1. Detect non-interactive / network-logon base tokens in the legacy restricted-token path.
2. Fall back to the elevated sandbox-user runner when elevated setup artifacts are already present.
3. If that fallback is not available, fail with a targeted error instead of silently returning empty output.

At the time of this handoff, these edits are committed in the branch history.

The relevant code changes are:

- `codex-rs/windows-sandbox-rs/src/lib.rs`
- `codex-rs/windows-sandbox-rs/src/token.rs`
- `codex-rs/windows-sandbox-rs/src/launch_guard.rs`

This is now validated on Umbra's Windows SSH / network-logon repro for the
setup-missing case. The remaining optional follow-up is a fresh local
interactive recheck with the patched binary, but the original failing path is
already covered.

## Verified Result On Umbra

On March 17, 2026, I rebuilt the branch as a Windows debug artifact from commit
`ce5781c` using GitHub Actions and re-ran the SSH / network-logon probe on
Umbra.

Host state before the repro:

- `C:\\Users\\codyr\\.codex\\.sandbox\\setup_marker.json` was missing
- `C:\\Users\\codyr\\.codex\\.sandbox-secrets\\sandbox_users.json` was missing

Observed result:

- every probe command returned the new targeted setup-required error with `EXIT=1`
- that included direct `powershell.exe`, direct `dotnet.exe --info`, direct `cmd.exe /c echo ok`, direct `reg.exe query HKCU\\Environment`, and the child `powershell.exe` / `dotnet.exe` / `whoami /groups` commands through sandboxed `cmd.exe`
- the old SSH / network-logon failure pattern did not reproduce
- `C:\\Users\\codyr\\.codex\\.sandbox\\sandbox.log` showed repeated:
  `legacy sandbox: session_id=0 interactive=false remote_interactive=false network=true -> elevated setup required`

Result classification:

- `clean targeted failure because setup is missing`

## Expected Outcome After Patch

When this branch is tested again from the same SSH / network-logon context,
there are now four meaningful result classes:

1. `fixed via elevated fallback`
   This is the expected success case when elevated sandbox setup has already completed on the machine.
   In that case, the legacy restricted-token path should detect the non-interactive / network-logon token and switch over to the elevated sandbox-user runner before launch.
   The probe commands that previously failed under SSH, especially sandboxed `powershell.exe` and sandboxed `dotnet.exe --info`, should no longer fail with the old legacy symptom pattern.

2. `clean targeted failure because setup is missing`
   This is the expected failure case when the machine has not yet completed elevated sandbox setup.
   In that case, the branch should fail early with the new targeted message:
   `Windows restricted-token sandbox cannot launch reliably from this non-interactive or network-logon session until elevated Windows sandbox setup has completed. Run Codex once from an interactive administrator session to finish elevated setup, or configure windows.sandbox = "elevated".`
   This is still considered a correct result for this patch because it replaces the old silent / misleading failure mode with an actionable one.

3. `still reproduces old legacy failure`
   This is the regression case.
   If the SSH / network-logon repro still produces the old symptom pattern, for example:
   - sandboxed `powershell.exe` exiting `-1073741502`
   - sandboxed `dotnet.exe` showing the same CoreCLR bind / load failures
   - empty stdout / stderr where the new targeted error should have appeared
   then the patch did not intercept the failing path and more work is needed.

4. `elevated fallback attempted but the elevated runner still failed`
   This is distinct from the old legacy failure.
   In this case, the new guard did fire and did route into the elevated
   sandbox-user runner, but that runner still failed for some other reason.
   That means the legacy guard is probably working, but the elevated path has
   its own failure that must be diagnosed separately.

In short:

- success means elevated fallback engages and the commands run
- acceptable failure means the new targeted setup-required error appears
- a separate elevated-runner failure means fallback engaged but the alternate path still broke
- unacceptable failure means the old legacy behavior is still happening

## New Logging Signal

The patch adds explicit log messages in the legacy capture path so the verifier can tell which branch executed.

Check the sandbox log at:

- `CODEX_HOME/.sandbox/sandbox.log`

Look for one of these new messages:

- `legacy sandbox: session_id=... interactive=... remote_interactive=... network=... -> using elevated sandbox-user runner`
- `legacy sandbox: session_id=... interactive=... remote_interactive=... network=... -> elevated setup required`
- `legacy sandbox: token inspection failed; continuing legacy runner: ...`

Interpretation:

- `using elevated sandbox-user runner` means the new fallback path engaged
- `elevated setup required` means the patch correctly identified the bad token context but setup artifacts were missing
- `token inspection failed; continuing legacy runner` means the guard itself could not classify the token, which is useful if the old failure still reproduces

If the SSH verifier sees the old failure pattern, the presence or absence of one of these log lines is important evidence.

## Code Pointers

- `codex-rs/windows-sandbox-rs/src/lib.rs`
- `codex-rs/windows-sandbox-rs/src/process.rs`
- `codex-rs/windows-sandbox-rs/src/token.rs`
- `codex-rs/windows-sandbox-rs/src/elevated_impl.rs`
- `codex-rs/core/src/exec.rs`
- `codex-rs/core/src/shell.rs`

## Recommended Local Checks

Run the probe script from a local Windows console first.

If your current directory is the repo root, use:

```bat
set CODEX_BIN=C:\path\to\codex.exe
docs\investigations\issue-14015-windows\network_logon_probe.cmd
```

If you copied the handoff files into a standalone Windows folder such as
`C:\Users\codyr\Downloads\issue-14015-handoff\`, use:

```bat
set CODEX_BIN=C:\path\to\codex.exe
network_logon_probe.cmd
```

Then compare that with:

- the same script from your SSH / remote context
- the interactive TUI repro for `echo $env:PATH`
- the interactive TUI repro for `cmd.exe /c echo %PATH%`

If the local console succeeds while SSH fails, that confirms the current boundary is logon-context-specific.

If the local console also fails, then the public issue comment can be broadened beyond the SSH / network-logon scope.

## SSH Verification Checklist

For the other agent, the practical goal is not just "run the probe again" but "classify which code path executed."

Recommended SSH verification sequence:

1. Confirm whether elevated sandbox setup has already been completed on the host.
   If setup is complete, the expected result is fallback-driven success.
   If setup is not complete, the expected result is the new targeted setup-required failure.

2. Record the commit hash being tested.
   If the verifier is using an uncommitted checkout instead, note that explicitly.

3. Run the probe using the right path form for the current working directory:

```bat
set CODEX_BIN=C:\path\to\codex.exe
docs\investigations\issue-14015-windows\network_logon_probe.cmd
```

Or, from a standalone handoff folder:

```bat
set CODEX_BIN=C:\path\to\codex.exe
network_logon_probe.cmd
```

4. Capture the exact stdout / stderr and exit codes for:
   - direct sandboxed `powershell.exe`
   - direct sandboxed `dotnet.exe --info`
   - child `powershell.exe` via sandboxed `cmd.exe`
   - child `dotnet.exe` via sandboxed `cmd.exe`

5. Inspect `CODEX_HOME/.sandbox/sandbox.log` immediately after the run and record whether one of the new log markers appeared.

6. Report the result in one of these forms:
   - `fixed via elevated fallback`
   - `clean targeted failure because setup is missing`
   - `elevated fallback attempted but the elevated runner still failed`
   - `still reproduces old legacy failure`

7. In the verification notes, explicitly record:
   - whether the patch was committed or still local-only
   - whether elevated sandbox setup was already complete
   - which of the four result classes occurred
   - whether the sandbox log showed the new marker lines

8. If the result is still the old failure, include:
   - whether elevated setup was present
   - the exact failing command(s)
   - whether the sandbox log showed `using elevated sandbox-user runner`, `elevated setup required`, or `token inspection failed; continuing legacy runner`
   - whether the failure happened only for the initial direct launch or also for child commands launched inside sandboxed `cmd.exe`

That should give enough information to determine whether the remaining issue is:

- the token-context detector not firing
- elevated setup artifacts missing
- elevated fallback launching but still inheriting the bad behavior
- elevated fallback launching but the elevated runner itself failing for a separate reason
- or an unrelated Windows sandbox regression

## Current Draft Comment

See [draft-comment.md](./draft-comment.md).
