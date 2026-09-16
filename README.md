# macOS Dev Cache Cleaner

A local-only macOS utility for developer machines with heavy AI/dev stacks.
Two tools in one: a **GUI app** (Tauri/Rust) and a standalone **CLI binary**.

## Current functionality

The GUI currently provides two tabs:

- **System Cache Cleaner** discovers known Homebrew, JetBrains, AI/LLM, Apple
  developer, Python, Node, Rust, Java/Android, Docker, Go, Ruby, Dart, PHP,
  Cloud/DevOps, and macOS log caches. It supports search, risk and category
  filters, directory browsing, dry-run plans, safe-default selection, and
  selective cleanup.
- **Project Artifact Scanner** recursively scans a chosen root such as
  `~/Code` for supported build and dependency artifacts, reports project,
  artifact type, size, and age. Android projects include Gradle/module
  `build` output, `.cxx`, `.externalNativeBuild`, `captures`, `.navigation`,
  and `.kotlin` leftovers. Selected artifacts can be moved to Trash or
  permanently deleted.

Use **Choose folder…** beside the scanner root field to select a directory
with the native macOS folder picker, or enter a path manually.
Click any results-table column heading to sort it; click again to reverse the
order. The Size, Last modified, and Age columns start with largest/newest or
oldest-first views as appropriate. The sort dropdown remains available for
the same fields.
The scanner also shows the combined disk space for the currently selected
artifacts and updates it as selections change.
Artifact paths in the results are shown relative to the selected scan root;
click a project/folder entry to toggle its checkbox.
Scanning and deletion show live progress in the scanner log, including large
directory size measurements, while the application remains interactive.
Completed artifacts stream into the results table in batches during a scan;
the final result is sorted and rendered again when the scan finishes.

The standalone `cache-cleaner` CLI supports scanning and listing targets,
JSON output, dry runs, selective or category-based cleanup, permanent-delete
and command opt-ins, and a macOS LaunchAgent service for scheduled cleanup.
File cleanup uses Trash by default and requires explicit confirmation unless
the CLI `--yes` option is supplied.

The GUI’s **How searches work** tab explains the fixed system paths, glob
patterns, project traversal rules, Android markers, filters, and cleanup
parameters used by the two scanners.
The top-right status badge reports the main Tauri process’s resident memory
usage and refreshes every three seconds.

---

## Contents

- [What it cleans](#what-it-cleans)
- [Scripts quick reference](#scripts-quick-reference)
- [Requirements](#requirements)
- [Development](#development)
- [Building the GUI app](#building-the-gui-app)
- [Installing the GUI app](#installing-the-gui-app)
- [Deploying the CLI binary](#deploying-the-cli-binary)
- [CLI usage](#cli-usage)
- [Automated cleaning service](#automated-cleaning-service)
- [Safety model](#safety-model)
- [Extending targets](#extending-targets)

---

## What it cleans

### System Cache Cleaner tab

| Category | What |
|---|---|
| Homebrew | Download cache, optional `brew cleanup -s` |
| JetBrains | Logs, tmp, JCEF cache, indexes, system caches, plugin cache, heap dumps |
| AI / LLM | Ollama logs, Hugging Face, PyTorch, MLX, LM Studio, ComfyUI caches |
| Apple dev | Xcode DerivedData, SwiftPM, Instruments, Simulator logs |
| Python | pip, uv, Poetry, conda/mamba caches |
| Node / JS | npm, Yarn, pnpm |
| Rust | Cargo registry and git caches |
| Java / Android | Gradle build cache, Android Studio caches |
| Docker | Logs, optional builder prune, optional system prune |
| Go, Ruby, Dart, PHP | Vendor and cache dirs |
| Cloud / DevOps | Terraform plugins, Pulumi, AWS CDK, Helm |
| macOS | Unified log archive |

### Project Artifact Scanner tab

Recursively walks a chosen root directory (e.g. `~/Code`) to find per-project
artifact directories: `node_modules`, `.venv`, Rust/Maven/Gradle `target`,
Android `build`/`.cxx` output, `.externalNativeBuild`, `.next`, `.terraform`,
CocoaPods `Pods`, and 40+ more patterns.
Results show project name, artifact type, size, and last-modified age
with colour-coded staleness chips.

---

## Scripts quick reference

| Script | Purpose |
|---|---|
| `run_dev.sh` | Start hot-reload development environment |
| `build_app.sh` | Build the production `.app` and `.dmg` |
| `install_app.sh` | Install / uninstall the `.app` to `/Applications` |
| `deploy.sh` | Build and package the CLI binary as a distributable archive |

---

## Requirements

```bash
xcode-select --install          # macOS SDK headers
brew install node               # Node 18+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust
```

Verify:

```bash
node --version   # v18+
cargo --version  # 1.75+
```

---

## Development

### `run_dev.sh`

Starts the Vite dev server and the Tauri hot-reload process together.
Checks for Cargo and Node before starting; runs `npm install` automatically
if `node_modules` is missing.

```bash
./run_dev.sh
```

Equivalent manual steps:

```bash
npm install
npm run tauri:dev
```

The window opens automatically once Rust compilation finishes (~30 s on first
run, ~3 s on subsequent runs). Frontend edits hot-reload instantly;
Rust changes trigger a recompile.

---

## Building the GUI app

### `build_app.sh`

Runs `npm install` if needed, then a full production build that compiles
the Rust backend with LTO and produces two bundle artefacts.

```bash
./build_app.sh
```

Output:

```
src-tauri/target/release/bundle/
  macos/macOS Dev Cache Cleaner.app   ← self-contained app bundle (~4 MB)
  dmg/macOS Dev Cache Cleaner_*.dmg   ← distributable disk image
```

The build takes roughly 45–90 seconds on Apple Silicon.
Icons are read from `src-tauri/icons/` (generated from `src-tauri/icons/icon.png`
via `npx tauri icon <source.png>`).

---

## Installing the GUI app

### `install_app.sh`

Installs the built `.app` to `/Applications`, or uninstalls it.
Strips the Gatekeeper quarantine flag automatically so the app opens
without a security warning on the first launch.

#### Install (requires a prior build)

```bash
./install_app.sh
```

Copies `…/bundle/macos/macOS Dev Cache Cleaner.app` → `/Applications/`,
strips `com.apple.quarantine`, and refreshes the Launch Services database.

#### Build then install in one step

```bash
./install_app.sh --build
```

Runs `build_app.sh` internally, then installs.

#### Open the `.dmg` for manual drag-to-Applications

```bash
./install_app.sh --dmg
```

Opens the disk image in Finder. Drag the app icon into the Applications
shortcut as usual.

#### Uninstall

```bash
./install_app.sh --uninstall
```

Moves `/Applications/macOS Dev Cache Cleaner.app` to Trash via Finder.

#### All flags

```
./install_app.sh [--build] [--dmg] [--uninstall]

  (no flags)    Install already-built .app to /Applications
  --build       Build first, then install
  --dmg         Open the .dmg for manual drag install
  --uninstall   Move installed .app to Trash
```

---

## Deploying the CLI binary

### `deploy.sh`

Builds the standalone `cache-cleaner` CLI binary (no GUI, no Tauri runtime)
and packages it with `install.sh` and `uninstall.sh` into a self-contained
`.tar.gz` archive suitable for copying to any macOS machine.

```bash
./deploy.sh                 # release build, current architecture (arm64)
./deploy.sh --debug         # debug build — faster compile, larger binary
./deploy.sh --universal     # fat binary: arm64 + x86_64 (needs both Rust targets)
```

Output:

```
dist/
  cache-cleaner-0.1.0-macos-arm64.tar.gz   (~400 KB)
    cache-cleaner          ← standalone CLI binary
    install.sh             ← installs binary + optional service setup
    uninstall.sh           ← removes binary and service
```

#### Installing from the archive (local or remote)

```bash
tar xzf cache-cleaner-0.1.0-macos-arm64.tar.gz
cd cache-cleaner-0.1.0-macos-arm64

./install.sh                        # install to ~/.local/bin
./install.sh --system               # install to /usr/local/bin  (sudo)
./install.sh --dir /my/bin          # custom directory
./install.sh --service              # install binary + set up weekly LaunchAgent
./install.sh --service daily        # weekly (default) | daily
./install.sh --service-hour 2       # run at 02:00 instead of 03:00
./install.sh --allow-commands       # let the service run brew/docker commands too
```

The install script also strips the macOS quarantine flag and checks whether
the install directory is in `$PATH`, printing the export line to add if not.

#### Uninstalling

```bash
./uninstall.sh
```

Stops and removes the LaunchAgent service if active, then removes the binary
from all standard locations (`~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin`).

#### Remote deploy (example)

```bash
scp dist/cache-cleaner-0.1.0-macos-arm64.tar.gz user@host:~/
ssh user@host '
  tar xzf ~/cache-cleaner-0.1.0-macos-arm64.tar.gz
  ~/cache-cleaner-0.1.0-macos-arm64/install.sh --service
'
```

---

## CLI usage

```
cache-cleaner <COMMAND> [OPTIONS]

Commands:
  scan       Scan and print all found caches  (default when no command given)
  list       List every known target with its ID and metadata
  dry-run    Preview what would be cleaned without touching anything
  clean      Clean the selected caches
  permissions Check and fix macOS privacy permissions (Full Disk Access)
  service    Manage the automated LaunchAgent service
  help       Print help for a command
```

### Common workflows

```bash
# See what's on disk right now
cache-cleaner scan

# Preview the safe defaults
cache-cleaner dry-run --defaults

# Clean safe defaults, move to Trash
cache-cleaner clean --defaults --yes

# Clean a specific category permanently
cache-cleaner clean --category "Python" --permanent --yes

# List available target IDs
cache-cleaner list

# Clean specific targets by ID
cache-cleaner clean --ids jetbrains-logs,jetbrains-heap-dumps --yes

# JSON output for scripting
cache-cleaner scan --json | jq '.items[] | select(.sizeBytes > 1073741824)'
```

### Selection flags (`dry-run` / `clean`)

| Flag | Selects |
|---|---|
| `--defaults` | Safe-default targets (selected by default in the GUI) |
| `--low-risk` | All low-risk targets |
| `--all` | Every known target |
| `--ids a,b,c` | Specific target IDs (from `list`) |
| `--category X` | Further restrict to a category substring |

### Clean options

| Flag | Effect |
|---|---|
| `--permanent` | Delete permanently instead of moving to Trash |
| `--allow-commands` | Allow `brew`, `docker`, `pnpm`, etc. command targets |
| `--include-high-risk` | Include high-risk targets (model stores, etc.) |
| `--yes` / `-y` | Skip the confirmation prompt (for scripts) |

---

## Automated cleaning service

Managed via `cache-cleaner service`:

```bash
# Install weekly cleanup at 03:00, move to Trash
cache-cleaner service install

# Install daily at 02:00, permanent delete, allow brew/docker commands
cache-cleaner service install --schedule daily --hour 2 \
  --permanent --allow-commands

# Override the binary path (important if the binary will move)
cache-cleaner service install \
  --bin ~/.local/bin/cache-cleaner \
  --schedule weekly

# Check status and tail recent log
cache-cleaner service status

# Remove the service
cache-cleaner service uninstall
```

The service creates a LaunchAgent plist at
`~/Library/LaunchAgents/com.macos-dev-cache-cleaner.plist` and logs to
`~/Library/Logs/dev-cache-cleaner-autorun.log`.

> **Note:** always pass `--bin <absolute-path>` pointing to a stable install
> location. The default uses `current_exe()`, which may point inside a Cargo
> build directory that gets wiped by `cargo clean`.

---

## Safety model

- **Trash by default.** All file-system operations move to Trash; you can
  recover anything before emptying it.
- **Commands are opt-in.** `brew cleanup`, `docker prune`, etc. are disabled
  unless "Allow commands" is checked (GUI) or `--allow-commands` is passed (CLI).
- **High-risk is opt-in.** Ollama model stores, Docker system prune, and JetBrains
  installed plugins require explicit opt-in.
- **No shell interpolation.** Commands run via `std::process::Command` with
  fixed arguments — no user input is ever interpolated into a shell.
- **Confirmation required.** The GUI asks you to type `CLEAN` or `DELETE`;
  the CLI asks the same unless `--yes` is supplied.

---

## Extending targets

Add a `TargetSpec` to `specs()` in `src-tauri/src/lib.rs`.

**Path target:**

```rust
TargetSpec {
    id: "my-tool-cache",
    label: "My tool cache",
    category: "My category",
    risk: Risk::Low,
    default_selected: true,
    note: "Recreated on next run.",
    kind: TargetKind::Path {
        patterns: vec!["~/.cache/my-tool", "~/Library/Caches/my-tool"],
    },
}
```

Glob patterns are supported: `~/Library/Caches/JetBrains/*/index` expands
to one entry per installed IDE.

**Command target:**

```rust
TargetSpec {
    id: "my-tool-prune",
    label: "My tool prune",
    category: "My category",
    risk: Risk::Medium,
    default_selected: false,
    note: "Runs the official prune command.",
    kind: TargetKind::Command {
        executable: "my-tool",
        dry_args: vec!["cache", "list"],
        clean_args: vec!["cache", "prune", "--force"],
        dry_note: "my-tool cache list",
    },
}
```

After editing, run `./run_dev.sh` — the Rust backend recompiles automatically.
