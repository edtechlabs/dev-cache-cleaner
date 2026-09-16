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
