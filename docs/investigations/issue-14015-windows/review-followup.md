# Review Follow-Up For Next Agent

This note captures the concrete issues found during handoff review. Treat these
as the next things to fix before trusting the updated README as the source of
truth.

## What You Need To Tend To

1. The README currently overstates what is committed.

The current README says the patch is implemented in the branch, but the actual
Windows sandbox code changes are only present as local working-tree edits in the
WSL checkout:

- `codex-rs/windows-sandbox-rs/src/lib.rs`
- `codex-rs/windows-sandbox-rs/src/token.rs`
- `codex-rs/windows-sandbox-rs/src/launch_guard.rs`

You need to do one of these:

- commit the code changes so the branch and the README agree, or
- tone the README back down so it says the patch exists only in the local
  working tree and is not yet committed

Do not leave the README claiming more than the branch actually contains.

2. The README's post-patch outcome list is incomplete.

Right now it lists:

- fixed via elevated fallback
- clean targeted failure because setup is missing
- still reproduces old legacy failure

Add a fourth outcome for:

- elevated fallback attempted but the elevated runner still failed

That is a real possible result because the new legacy guard can route into the
elevated runner, and that runner can still fail for reasons unrelated to the
old legacy symptom.

3. The probe-path instructions need to distinguish repo-root usage from the
standalone Windows handoff folder.

The README currently tells the verifier to run:

`docs\\investigations\\issue-14015-windows\\network_logon_probe.cmd`

That is only correct when the current directory is the repo root.

Also document the standalone Windows-folder form:

- from the repo root:
  `docs\\investigations\\issue-14015-windows\\network_logon_probe.cmd`
- from `C:\\Users\\codyr\\Downloads\\issue-14015-handoff\\`:
  `network_logon_probe.cmd`

4. After fixing the docs mismatch, rerun verification and classify the result.

The next verification pass should explicitly record:

- whether the patch was committed or still local-only
- whether elevated sandbox setup was already complete
- whether the SSH / network-logon repro hit:
  - elevated fallback success
  - targeted setup-required failure
  - elevated fallback attempted but failed
  - old legacy failure still reproducing
- whether the sandbox log shows the new marker lines

## Short Version

Before continuing:

- make the README honest about commit state
- add the missing elevated-runner-failed outcome
- fix the probe-path instructions
- then rerun verification

Only after that should the public issue comment be revised again.
