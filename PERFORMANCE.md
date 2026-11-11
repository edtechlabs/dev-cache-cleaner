# Windows responsiveness profile

Measured on the development Windows machine using the release build, before and
after the responsiveness fix. These are local measurements, not performance
guarantees for other machines.

## Findings and changes

The memory badge invoked `powershell.exe` every three seconds and synchronously
waited on the Tauri window thread. The old polling body took 234–266 ms in seven
initial measurements. Cache discovery, cleanup previews, cleaning, and recursive
tree sizing also ran on that thread, blocking window messages during disk work.
[Tauri documents this behavior for synchronous commands](https://v2.tauri.app/develop/calling-rust/).

Those commands now use `spawn_blocking` workers. The CLI retains synchronous
entry points. Windows memory polling now calls
[GetProcessMemoryInfo](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo)
directly, without creating a child process. The existing async project scanner
and deletion workers remain in place.

Artifact rendering also constructed a date formatter per row and repeated the
same filter/sort while updating statistics. It now shares one formatter and
reuses the filtered rows for statistics.

## Results

| Measurement | Before | After |
| --- | ---: | ---: |
| Idle native window dispatch p95 | 1,011.779 ms | 0.479 ms |
| Worst native window dispatch | 1,023.005 ms | 9.923 ms |
| One-second dispatch timeouts in 12 seconds | 11 | 0 |
| Samples taking at least 50 ms | 11 | 0 |
| Render HTML for 3,000 artifact rows | 184.026 ms | 12.800 ms |

The window probe ran sequentially against a saved original executable and the
rebuilt executable, after compilation finished. It sends `WM_NULL` every 20 ms,
with a one-second timeout, after a two-second startup wait. The old build yielded
50 samples and the new build 378; blocked probes and scheduler timing reduce the
sample count. Non-timeout failed calls are reported separately in the raw JSON.
This tests native message-loop responsiveness, not browser frame rate.

The row benchmark ran the original and modified row-rendering functions under
Node against the same 3,000 synthetic artifacts and checked identical HTML.
It excludes DOM insertion, browser layout, and painting.

The manual Rust polling benchmark measured seven PowerShell calls at 295–521 ms
each under build load, versus 0.212 microseconds per native call averaged over
10,000 calls in a debug test binary. This is a polling microbenchmark, not IPC
latency. It is deliberately excluded from ordinary tests.

## Reproduce

Build and close any existing app instances, then run:

```powershell
.\build_app.bat
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/profile-windows.ps1
```

The probe launches a separate app instance, prints JSON, and closes only its own
process. Use `-Seconds 30` for a longer capture or `-Executable <path>` to compare
another release binary. Local captures belong in `.codex-logs/`, not Git.

For the memory benchmark:

```powershell
. .\dev_env.ps1
cargo test --manifest-path src-tauri/Cargo.toml --lib profile_memory_poll -- --ignored --nocapture
```

Validation: Rust check and formatting, five regular Windows tests, the manual
polling benchmark, frontend build, release build, both Windows installers, and
before/after native window probes passed. No cleanup was executed.

Large result sets still create all visible table rows. This profile does not
establish total scan throughput or responsiveness for every possible project
tree; those need a representative user scan and a browser layout trace if lag
persists after this fix.
