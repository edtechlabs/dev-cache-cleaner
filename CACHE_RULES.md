# Cache Rules

This file documents the first rule set used by the Rust scanner.

## Risk levels

- **Low**: logs/download caches that tools can usually recreate quickly.
- **Medium**: caches that are safe to rebuild but may cause slow first runs or large re-downloads.
- **High**: model stores or pruning commands that can remove large working state.

## Default-selected targets

- Homebrew download cache
- JetBrains logs
- Ollama logs
- pip cache
- uv cache
- Poetry cache
- npm cache
- Yarn cache
- Docker Desktop logs
- user URLSession temporary downloads
- npm debug logs, Bun, Deno, node-gyp and Electron download caches
- pre-commit environments
- VS Code caches (VSIX cache, code cache, GPU/HTTP caches)
- Windows: leftover installer temp folders (Docker Desktop updates, VS Code
  installer, pip/npm/Chocolatey temp), crash dumps and the D3D shader cache

## Not default-selected

- Homebrew cleanup command
- JetBrains IDE caches
- Xcode DerivedData
- SwiftPM caches
- Ollama model store
- Hugging Face cache
- Torch cache
- MLX cache
- Conda/Mamba package caches
- pnpm commands/raw stores
- Cargo registry/git caches
- Gradle build caches
- Android Studio caches
- Docker builder prune
- Docker system prune
- Playwright, Puppeteer and Cypress browser downloads
- Codex runtimes, `.codex/.tmp` and `.codex/cache`
- Edge and Chrome HTTP/code/GPU caches (close the browsers first)
- Visual Studio component caches and the WinGet cache

## Project scanner rules

Rules live in `src/project-artifact-rules.json` (version 2). Markers in
`requiresOutside`, `requiresOutsideAny` and `requiresInside` may be a file
name or a `*.ext` pattern such as `*.csproj`. Added in version 2: .NET
`bin`/`obj`, Visual Studio `.vs`, Nx, Astro, Docusaurus, Expo, Vercel,
Wrangler, Nuxt `.output`, Hypothesis, Jupyter checkpoints, pytype, CMake
`cmake-build-*`, SwiftPM `.build`, Zig `zig-out`, Serverless and AWS SAM.
Saved rules in the app are merged with new built-in rules when this version
increases; custom and edited rules are kept.

## Deliberately not targeted

- Docker Desktop's WSL disk image (`%LOCALAPPDATA%/Docker/wsl`): deleting it
  removes all images and volumes; compact it from Docker Desktop instead.
- The whole `%TEMP%` folder: mostly random names that may be in use.
- AI tool session history (`~/.claude`, `~/.codex/sessions`).

## Suggested future modules

1. Project-root scanner for selected folders only, with rules for:
   - `target/`
   - `.next/cache/`
   - `.turbo/`
   - `.pytest_cache/`
   - `.mypy_cache/`
   - `.ruff_cache/`
   - `node_modules/.cache/`
2. Model inventory view:
   - `ollama list`
   - Hugging Face snapshots grouped by model name
   - Docker image list grouped by repository/tag
3. Scheduled reminder mode, not auto-clean mode.
4. JSON export of dry-run plans for audit trails.
