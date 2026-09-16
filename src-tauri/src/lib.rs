use chrono::{DateTime, Utc};
use glob::glob;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
enum TargetKind {
    Path { patterns: Vec<&'static str> },
    Command {
        executable: &'static str,
        dry_args: Vec<&'static str>,
        clean_args: Vec<&'static str>,
        dry_note: &'static str,
    },
}

#[derive(Debug, Clone)]
struct TargetSpec {
    id: &'static str,
    label: &'static str,
    category: &'static str,
    risk: Risk,
    default_selected: bool,
    note: &'static str,
    kind: TargetKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverOptions {
    pub include_missing: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheItem {
    pub id: String,
    pub label: String,
    pub category: String,
    pub risk: Risk,
    pub kind: String,
    pub path: Option<String>,
    pub command: Option<String>,
    pub exists: bool,
    pub selected_by_default: bool,
    pub size_bytes: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub modified_rfc3339: Option<String>,
    pub note: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverReport {
    pub generated_at: String,
    pub total_bytes: u64,
    pub items: Vec<CacheItem>,
    pub category_totals: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanRequest {
    pub selected_ids: Vec<String>,
    pub permanent: bool,
    pub allow_commands: bool,
    pub include_high_risk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAction {
    pub id: String,
    pub label: String,
    pub category: String,
    pub risk: Risk,
    pub kind: String,
    pub path: Option<String>,
    pub command: Option<String>,
    pub size_bytes: u64,
    pub action: String,
    pub status: String,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReport {
    pub generated_at: String,
    pub dry_run: bool,
    pub permanent: bool,
    pub total_bytes: u64,
    pub actions: Vec<PlannedAction>,
    pub warnings: Vec<String>,
}

// ── Project artifact scanner types ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArtifact {
    pub path: String,
    pub dir_name: String,
    pub artifact_kind: String,
    pub project_name: String,
    pub project_path: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub modified_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScanOptions {
    pub root: String,
    pub min_size_bytes: Option<u64>,
    pub max_depth: Option<usize>,
    pub artifact_paths: Option<Vec<String>>,
    pub artifact_rules: Option<Vec<ArtifactRule>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRule {
    pub directory_name: String,
    pub artifact_kind: String,
    #[serde(default)]
    pub locations: Vec<String>,
    #[serde(default)]
    pub requires_outside: Vec<String>,
    #[serde(default)]
    pub requires_outside_any: Vec<String>,
    #[serde(default)]
    pub requires_inside: Vec<String>,
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub url: Option<String>,
}

fn default_rule_enabled() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScanReport {
    pub root: String,
    pub artifacts: Vec<ProjectArtifact>,
    pub total_bytes: u64,
    pub artifact_count: u64,
    pub project_count: u64,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteArtifactsRequest {
    pub paths: Vec<String>,
    pub permanent: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectProgress {
    phase: &'static str,
    current: u64,
    total: Option<u64>,
    path: String,
    found: u64,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectArtifactBatch {
    root: String,
    artifacts: Vec<ProjectArtifact>,
}

enum ArtifactSizingEvent {
    Progress { path: String, files_scanned: u64 },
    Complete { index: usize, artifact: Option<ProjectArtifact> },
}

fn emit_project_progress(app: Option<&AppHandle>, progress: ProjectProgress) {
    if let Some(app) = app {
        let _ = app.emit("project-progress", progress);
    }
}

fn emit_project_artifact_batch(
    app: Option<&AppHandle>,
    root: &Path,
    artifacts: &[ProjectArtifact],
) {
    if let Some(app) = app {
        let _ = app.emit(
            "project-artifact-batch",
            ProjectArtifactBatch {
                root: root.display().to_string(),
                artifacts: artifacts.to_vec(),
            },
        );
    }
}

// ── Tree viewer type ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub entry_count: u64,
}

#[derive(Debug, Default, Clone)]
struct PathStats {
    exists: bool,
    size_bytes: u64,
    file_count: u64,
    dir_count: u64,
    latest_modified: Option<SystemTime>,
}

fn specs() -> Vec<TargetSpec> {
    vec![
        TargetSpec {
            id: "brew-download-cache",
            label: "Homebrew download cache",
            category: "Homebrew",
            risk: Risk::Low,
            default_selected: true,
            note: "Downloaded formula and cask archives. Homebrew can re-download them.",
            kind: TargetKind::Path {
                patterns: vec!["$BREW_CACHE", "~/Library/Caches/Homebrew"],
            },
        },
        TargetSpec {
            id: "brew-cleanup-command",
            label: "Homebrew cleanup command",
            category: "Homebrew",
            risk: Risk::Medium,
            default_selected: false,
            note: "Runs brew cleanup -s. This removes old versions and stale downloads; not selected by default.",
            kind: TargetKind::Command {
                executable: "brew",
                dry_args: vec!["cleanup", "-s", "--dry-run"],
                clean_args: vec!["cleanup", "-s"],
                dry_note: "brew cleanup -s --dry-run",
            },
        },
        TargetSpec {
            id: "jetbrains-logs",
            label: "JetBrains IDE logs",
            category: "JetBrains",
            risk: Risk::Low,
            default_selected: true,
            note: "Log files written by all JetBrains IDEs. Safe to remove at any time.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Logs/JetBrains"],
            },
        },
        TargetSpec {
            id: "jetbrains-tmp",
            label: "JetBrains temp files",
            category: "JetBrains",
            risk: Risk::Low,
            default_selected: true,
            note: "Per-IDE temporary files. Safe to remove when no JetBrains IDE is running.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/JetBrains/*/tmp"],
            },
        },
        TargetSpec {
            id: "jetbrains-jcef-cache",
            label: "JetBrains JCEF browser cache",
            category: "JetBrains",
            risk: Risk::Low,
            default_selected: true,
            note: "Chromium engine cache used by the built-in IDE browser (HTTP cache, cookies, GPU shader cache). Rebuilt on next use.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/JetBrains/*/jcef_cache"],
            },
        },
        TargetSpec {
            id: "jetbrains-indexes",
            label: "JetBrains file indexes",
            category: "JetBrains",
            risk: Risk::Medium,
            default_selected: false,
            note: "Per-IDE file and symbol indexes. Rebuilt automatically but the first IDE startup after removal can take several minutes for large projects.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/JetBrains/*/index"],
            },
        },
        TargetSpec {
            id: "jetbrains-system-caches",
            label: "JetBrains system caches",
            category: "JetBrains",
            risk: Risk::Medium,
            default_selected: false,
            note: "Per-IDE system caches (VCS log, editor state, icon cache). Rebuilt on next IDE start; expect a slower first launch.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/JetBrains/*/caches"],
            },
        },
        TargetSpec {
            id: "jetbrains-plugin-cache",
            label: "JetBrains plugin cache",
            category: "JetBrains",
            risk: Risk::Medium,
            default_selected: false,
            note: "Cached plugin data extracted from plugin JARs. This is NOT the plugin installation — plugins remain installed. Re-populated on next IDE start.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/JetBrains/*/plugins"],
            },
        },
        TargetSpec {
            id: "jetbrains-installed-plugins",
            label: "JetBrains installed plugins",
            category: "JetBrains",
            risk: Risk::High,
            default_selected: false,
            note: "Actually installed plugin binaries for all JetBrains IDEs. Removing forces a full reinstall of every plugin from JetBrains Marketplace.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/JetBrains/*/plugins"],
            },
        },
        TargetSpec {
            id: "jetbrains-heap-dumps",
            label: "JetBrains heap dumps",
            category: "JetBrains",
            risk: Risk::Low,
            default_selected: true,
            note: "Java heap snapshots written to ~ when a JetBrains IDE crashes with OutOfMemoryError. Pure crash artifacts — each file is typically 2–8 GB.",
            kind: TargetKind::Path {
                patterns: vec!["~/java_error_in_*.hprof"],
            },
        },
        TargetSpec {
            id: "jetbrains-crash-logs",
            label: "JetBrains JBR crash logs",
            category: "JetBrains",
            risk: Risk::Low,
            default_selected: true,
            note: "JetBrains Runtime (JBR) and JVM crash log files written to ~. Small text files produced on IDE crash.",
            kind: TargetKind::Path {
                patterns: vec!["~/jbr_err_pid*.log", "~/hs_err_pid*.log"],
            },
        },
        TargetSpec {
            id: "xcode-derived-data",
            label: "Xcode DerivedData",
            category: "Apple dev tools",
            risk: Risk::Medium,
            default_selected: false,
            note: "Build products and indexes. Safe to rebuild, but may slow the next build.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Developer/Xcode/DerivedData"],
            },
        },
        TargetSpec {
            id: "swiftpm-cache",
            label: "Swift Package Manager caches",
            category: "Apple dev tools",
            risk: Risk::Medium,
            default_selected: false,
            note: "SwiftPM repositories/artifacts. Re-downloaded on demand.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/org.swift.swiftpm", "~/.swiftpm/cache"],
            },
        },
        TargetSpec {
            id: "ollama-logs",
            label: "Ollama logs",
            category: "AI / LLM",
            risk: Risk::Low,
            default_selected: true,
            note: "Ollama log files only. Does not remove models.",
            kind: TargetKind::Path {
                patterns: vec!["~/.ollama/logs", "~/Library/Logs/Ollama"],
            },
        },
        TargetSpec {
            id: "ollama-model-store",
            label: "Ollama model store",
            category: "AI / LLM",
            risk: Risk::High,
            default_selected: false,
            note: "Downloaded models and blobs. Removing this frees space but forces model re-download. Prefer `ollama rm <model>` for targeted cleanup.",
            kind: TargetKind::Path {
                patterns: vec!["~/.ollama/models"],
            },
        },
        TargetSpec {
            id: "huggingface-cache",
            label: "Hugging Face cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "HF models, datasets, snapshots, and metadata. Re-download may be large and slow.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/huggingface"],
            },
        },
        TargetSpec {
            id: "torch-cache",
            label: "PyTorch / Torch hub cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "Torch hub/checkpoint cache. Re-downloaded on demand.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/torch", "~/Library/Caches/torch"],
            },
        },
        TargetSpec {
            id: "mlx-cache",
            label: "MLX / MLX-LM cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "Apple MLX model/cache data if present. Re-download may be large.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/mlx", "~/.cache/mlx_lm"],
            },
        },
        TargetSpec {
            id: "pip-cache",
            label: "pip caches",
            category: "Python",
            risk: Risk::Low,
            default_selected: true,
            note: "Python wheel/download cache. pip can re-download packages.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/pip", "~/.cache/pip"],
            },
        },
        TargetSpec {
            id: "uv-cache",
            label: "uv caches",
            category: "Python",
            risk: Risk::Low,
            default_selected: true,
            note: "Astral uv package/cache directory. uv can re-download packages.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/uv", "~/Library/Caches/uv"],
            },
        },
        TargetSpec {
            id: "poetry-cache",
            label: "Poetry caches",
            category: "Python",
            risk: Risk::Low,
            default_selected: true,
            note: "Poetry artifact/cache data. Recreated on demand.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/pypoetry", "~/.cache/pypoetry"],
            },
        },
        TargetSpec {
            id: "conda-pkg-cache",
            label: "Conda/Mamba package caches",
            category: "Python",
            risk: Risk::Medium,
            default_selected: false,
            note: "Downloaded conda package caches. Does not remove active environments, but re-installs may re-download packages.",
            kind: TargetKind::Path {
                patterns: vec![
                    "~/miniconda3/pkgs",
                    "~/anaconda3/pkgs",
                    "~/mambaforge/pkgs",
                    "~/micromamba/pkgs",
                ],
            },
        },
        TargetSpec {
            id: "npm-cache",
            label: "npm cache",
            category: "Node / JS",
            risk: Risk::Low,
            default_selected: true,
            note: "npm content-addressable cache. npm can re-download packages.",
            kind: TargetKind::Path {
                patterns: vec!["~/.npm/_cacache", "~/Library/Caches/npm"],
            },
        },
        TargetSpec {
            id: "yarn-cache",
            label: "Yarn cache",
            category: "Node / JS",
            risk: Risk::Low,
            default_selected: true,
            note: "Yarn download cache. Yarn can re-download packages.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/Yarn", "~/.cache/yarn"],
            },
        },
        TargetSpec {
            id: "pnpm-store-prune",
            label: "pnpm store prune command",
            category: "Node / JS",
            risk: Risk::Low,
            default_selected: false,
            note: "Runs pnpm store prune. Safer than deleting the whole pnpm store.",
            kind: TargetKind::Command {
                executable: "pnpm",
                dry_args: vec!["store", "status"],
                clean_args: vec!["store", "prune"],
                dry_note: "pnpm store status",
            },
        },
        TargetSpec {
            id: "pnpm-store-paths",
            label: "pnpm store paths",
            category: "Node / JS",
            risk: Risk::Medium,
            default_selected: false,
            note: "Raw pnpm package store paths. Prefer the pnpm prune command when possible.",
            kind: TargetKind::Path {
                patterns: vec!["~/.pnpm-store", "~/Library/pnpm/store", "~/Library/Caches/pnpm"],
            },
        },
        TargetSpec {
            id: "cargo-registry-cache",
            label: "Cargo registry/git caches",
            category: "Rust",
            risk: Risk::Medium,
            default_selected: false,
            note: "Rust crate downloads and git checkouts. Cargo can re-download them.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cargo/registry/cache", "~/.cargo/git/db", "~/.cargo/git/checkouts"],
            },
        },
        TargetSpec {
            id: "gradle-build-cache",
            label: "Gradle build caches",
            category: "Java / Android",
            risk: Risk::Medium,
            default_selected: false,
            note: "Gradle build caches. Useful for Android/Java machines, but next builds may be slower.",
            kind: TargetKind::Path {
                patterns: vec!["~/.gradle/caches/build-cache-*", "~/.gradle/caches/modules-2/files-2.1"],
            },
        },
        TargetSpec {
            id: "android-studio-caches",
            label: "Android Studio caches",
            category: "Java / Android",
            risk: Risk::Medium,
            default_selected: false,
            note: "Android Studio caches. They can be rebuilt; first launch/build may be slower.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/Google/AndroidStudio*"],
            },
        },
        TargetSpec {
            id: "docker-desktop-logs",
            label: "Docker Desktop logs",
            category: "Docker",
            risk: Risk::Low,
            default_selected: true,
            note: "Docker Desktop logs only. Does not remove images, volumes, or containers.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Containers/com.docker.docker/Data/log"],
            },
        },
        TargetSpec {
            id: "docker-builder-prune",
            label: "Docker builder cache prune",
            category: "Docker",
            risk: Risk::Medium,
            default_selected: false,
            note: "Runs docker builder prune for build cache older than 7 days. Does not delete named volumes.",
            kind: TargetKind::Command {
                executable: "docker",
                dry_args: vec!["system", "df"],
                clean_args: vec!["builder", "prune", "--all", "--force", "--filter", "until=168h"],
                dry_note: "docker system df",
            },
        },
        TargetSpec {
            id: "docker-system-prune",
            label: "Docker system prune",
            category: "Docker",
            risk: Risk::High,
            default_selected: false,
            note: "Runs docker system prune -f. Can remove stopped containers, unused networks, dangling images, and build cache. Volumes are not pruned unless you add --volumes manually.",
            kind: TargetKind::Command {
                executable: "docker",
                dry_args: vec!["system", "df"],
                clean_args: vec!["system", "prune", "-f"],
                dry_note: "docker system df",
            },
        },
        TargetSpec {
            id: "general-macos-temp",
            label: "User temporary files",
            category: "macOS",
            risk: Risk::Low,
            default_selected: true,
            note: "User-level temporary files. The OS may recreate them.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/com.apple.nsurlsessiond/Downloads"],
            },
        },
        // ── Go ──────────────────────────────────────────────────────────────
        TargetSpec {
            id: "go-build-cache",
            label: "Go build cache",
            category: "Go",
            risk: Risk::Low,
            default_selected: true,
            note: "Go compiler build/test cache. Runs `go clean -cache`. Rebuilds are slower after cleaning.",
            kind: TargetKind::Command {
                executable: "go",
                dry_args: vec!["env", "GOCACHE"],
                clean_args: vec!["clean", "-cache"],
                dry_note: "go env GOCACHE",
            },
        },
        TargetSpec {
            id: "go-module-cache",
            label: "Go module download cache",
            category: "Go",
            risk: Risk::Medium,
            default_selected: false,
            note: "Downloaded Go module source trees. Runs `go clean -modcache`. Modules are re-downloaded on next build.",
            kind: TargetKind::Command {
                executable: "go",
                dry_args: vec!["env", "GOMODCACHE"],
                clean_args: vec!["clean", "-modcache"],
                dry_note: "go env GOMODCACHE",
            },
        },
        // ── Ruby / CocoaPods ────────────────────────────────────────────────
        TargetSpec {
            id: "cocoapods-download-cache",
            label: "CocoaPods download cache",
            category: "Ruby / CocoaPods",
            risk: Risk::Low,
            default_selected: true,
            note: "Downloaded pod source archives. Re-downloaded on next `pod install`.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/CocoaPods"],
            },
        },
        TargetSpec {
            id: "cocoapods-spec-repos",
            label: "CocoaPods spec repos",
            category: "Ruby / CocoaPods",
            risk: Risk::Medium,
            default_selected: false,
            note: "Local clones of CocoaPods/Trunk spec repos. Re-cloned on `pod install`; can be 1–3 GB.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cocoapods/repos"],
            },
        },
        TargetSpec {
            id: "rubygems-cache",
            label: "RubyGems download cache",
            category: "Ruby / CocoaPods",
            risk: Risk::Low,
            default_selected: true,
            note: "Downloaded .gem files cached by gem/rubygems. Re-downloaded on `gem install`.",
            kind: TargetKind::Path {
                patterns: vec!["~/.gem/cache", "~/.local/share/gem/cache"],
            },
        },
        TargetSpec {
            id: "bundler-cache",
            label: "Bundler gem cache",
            category: "Ruby / CocoaPods",
            risk: Risk::Low,
            default_selected: true,
            note: "Bundler gem download cache. Re-downloaded on `bundle install`.",
            kind: TargetKind::Path {
                patterns: vec!["~/.bundle/cache"],
            },
        },
        // ── Dart / Flutter ──────────────────────────────────────────────────
        TargetSpec {
            id: "pub-cache",
            label: "Dart / Flutter pub cache",
            category: "Dart / Flutter",
            risk: Risk::Medium,
            default_selected: false,
            note: "Pub package downloads and git packages. Re-downloaded on `flutter pub get` / `dart pub get`. Can be 1–5 GB for Flutter projects.",
            kind: TargetKind::Path {
                patterns: vec!["~/.pub-cache"],
            },
        },
        // ── Java / Android additions ────────────────────────────────────────
        TargetSpec {
            id: "gradle-wrapper-dists",
            label: "Gradle wrapper distributions",
            category: "Java / Android",
            risk: Risk::Low,
            default_selected: true,
            note: "Downloaded Gradle distribution zips and unpacked toolchains. Re-downloaded when a project requests a specific version.",
            kind: TargetKind::Path {
                patterns: vec!["~/.gradle/wrapper/dists"],
            },
        },
        TargetSpec {
            id: "maven-local-repo",
            label: "Maven local repository",
            category: "Java / Android",
            risk: Risk::Medium,
            default_selected: false,
            note: "Maven artifact cache in ~/.m2/repository. Re-downloaded from remote repos; large projects may take time.",
            kind: TargetKind::Path {
                patterns: vec!["~/.m2/repository"],
            },
        },
        // ── Build systems ───────────────────────────────────────────────────
        TargetSpec {
            id: "bazel-cache",
            label: "Bazel build cache",
            category: "Build systems",
            risk: Risk::Medium,
            default_selected: false,
            note: "Bazel action/analysis cache. Re-built on next build; first build after cleanup will be significantly slower.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/bazel", "/private/var/tmp/_bazel_*"],
            },
        },
        // ── AI / LLM additions ──────────────────────────────────────────────
        TargetSpec {
            id: "whisper-cache",
            label: "Whisper model cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "OpenAI Whisper downloaded model weights. Re-downloaded on next use.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/whisper"],
            },
        },
        TargetSpec {
            id: "keras-cache",
            label: "Keras model cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "Keras pretrained model weights and datasets. Re-downloaded on demand.",
            kind: TargetKind::Path {
                patterns: vec!["~/.keras"],
            },
        },
        TargetSpec {
            id: "tensorflow-cache",
            label: "TensorFlow cache",
            category: "AI / LLM",
            risk: Risk::Medium,
            default_selected: false,
            note: "TensorFlow Hub and saved model caches. Re-downloaded on demand.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/tensorflow"],
            },
        },
        TargetSpec {
            id: "jan-models",
            label: "Jan AI model store",
            category: "AI / LLM",
            risk: Risk::High,
            default_selected: false,
            note: "Jan AI downloaded models. Removing frees space but forces re-download.",
            kind: TargetKind::Path {
                patterns: vec!["~/jan/models"],
            },
        },
        TargetSpec {
            id: "lm-studio-models",
            label: "LM Studio model store",
            category: "AI / LLM",
            risk: Risk::High,
            default_selected: false,
            note: "LM Studio downloaded models. Removing frees space but forces re-download.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/lm-studio/models", "~/.lmstudio/models"],
            },
        },
        // ── Apple dev tools additions ───────────────────────────────────────
        TargetSpec {
            id: "coresimulator-caches",
            label: "iOS Simulator build caches",
            category: "Apple dev tools",
            risk: Risk::Low,
            default_selected: true,
            note: "CoreSimulator lookup/build caches. Regenerated automatically; does not delete simulator devices.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Developer/CoreSimulator/Caches"],
            },
        },
        TargetSpec {
            id: "coresimulator-logs",
            label: "iOS Simulator logs",
            category: "Apple dev tools",
            risk: Risk::Low,
            default_selected: true,
            note: "CoreSimulator log files. Safe to remove; can accumulate gigabytes over time.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Logs/CoreSimulator"],
            },
        },
        TargetSpec {
            id: "coresimulator-devices",
            label: "iOS Simulator devices",
            category: "Apple dev tools",
            risk: Risk::High,
            default_selected: false,
            note: "All CoreSimulator device images. Frees many GB but deletes all simulator data. Prefer `xcrun simctl delete unavailable` for targeted cleanup.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Developer/CoreSimulator/Devices"],
            },
        },
        TargetSpec {
            id: "ios-device-backups",
            label: "iOS device backups",
            category: "Apple dev tools",
            risk: Risk::High,
            default_selected: false,
            note: "Local iOS/iPadOS device backups created by Finder. Removing may cause data loss if the device is not backed up elsewhere.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/MobileSync/Backup"],
            },
        },
        TargetSpec {
            id: "instruments-cache",
            label: "Instruments trace cache",
            category: "Apple dev tools",
            risk: Risk::Low,
            default_selected: false,
            note: "Xcode Instruments cached traces and temporary profiling data.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Caches/com.apple.dt.Instruments"],
            },
        },
        // ── Cloud / DevOps ──────────────────────────────────────────────────
        TargetSpec {
            id: "terraform-plugin-cache",
            label: "Terraform provider plugin cache",
            category: "Cloud / DevOps",
            risk: Risk::Low,
            default_selected: true,
            note: "Cached Terraform provider binaries. Re-downloaded on next `terraform init`.",
            kind: TargetKind::Path {
                patterns: vec!["~/.terraform.d/plugin-cache", "~/.terraform.d/plugins"],
            },
        },
        TargetSpec {
            id: "kubectl-cache",
            label: "kubectl API discovery cache",
            category: "Cloud / DevOps",
            risk: Risk::Low,
            default_selected: true,
            note: "kubectl API discovery and HTTP response cache. Automatically regenerated on next API call.",
            kind: TargetKind::Path {
                patterns: vec!["~/.kube/cache", "~/.kube/http-cache"],
            },
        },
        TargetSpec {
            id: "helm-cache",
            label: "Helm chart cache",
            category: "Cloud / DevOps",
            risk: Risk::Low,
            default_selected: true,
            note: "Cached Helm chart repositories and downloads. Refreshed on `helm repo update`.",
            kind: TargetKind::Path {
                patterns: vec!["~/.cache/helm", "~/Library/Caches/helm"],
            },
        },
        TargetSpec {
            id: "ansible-tmp",
            label: "Ansible temp / control sockets",
            category: "Cloud / DevOps",
            risk: Risk::Low,
            default_selected: true,
            note: "Ansible temporary files and SSH ControlMaster sockets. Safe to remove when no playbooks are running.",
            kind: TargetKind::Path {
                patterns: vec!["~/.ansible/tmp", "~/.ansible/cp"],
            },
        },
        // ── IDEs ────────────────────────────────────────────────────────────
        TargetSpec {
            id: "vscode-logs",
            label: "VS Code logs",
            category: "IDEs",
            risk: Risk::Low,
            default_selected: true,
            note: "VS Code log files. Safe to remove.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/Code/logs"],
            },
        },
        TargetSpec {
            id: "vscode-workspace-storage",
            label: "VS Code workspace storage",
            category: "IDEs",
            risk: Risk::Low,
            default_selected: false,
            note: "Per-workspace extension state and search history for VS Code. VS Code recreates this on next open. May accumulate many GB from old/deleted projects.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/Code/User/workspaceStorage"],
            },
        },
        TargetSpec {
            id: "cursor-logs",
            label: "Cursor logs",
            category: "IDEs",
            risk: Risk::Low,
            default_selected: true,
            note: "Cursor IDE log files. Safe to remove.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/Cursor/logs"],
            },
        },
        TargetSpec {
            id: "cursor-workspace-storage",
            label: "Cursor workspace storage",
            category: "IDEs",
            risk: Risk::Low,
            default_selected: false,
            note: "Per-workspace extension state and search history for Cursor. Cursor recreates this on next open.",
            kind: TargetKind::Path {
                patterns: vec!["~/Library/Application Support/Cursor/User/workspaceStorage"],
            },
        },
        // ── PHP ─────────────────────────────────────────────────────────────
        TargetSpec {
            id: "composer-cache",
            label: "Composer package cache",
            category: "PHP",
            risk: Risk::Low,
            default_selected: true,
            note: "PHP Composer package download cache. Re-downloaded on next `composer install`.",
            kind: TargetKind::Path {
                patterns: vec!["~/.composer/cache", "~/Library/Caches/composer"],
            },
        },
    ]
}

#[tauri::command]
fn discover(options: DiscoverOptions) -> Result<DiscoverReport, String> {
    let include_missing = options.include_missing.unwrap_or(false);
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();

    for spec in specs() {
        match &spec.kind {
            TargetKind::Path { patterns } => {
                let expanded = expand_patterns(patterns, &mut warnings);
                if expanded.is_empty() && include_missing {
                    items.push(path_item_from_missing_spec(&spec));
                }

                for path in expanded {
                    if !seen_paths.insert(path.clone()) {
                        continue;
                    }
                    let stats = path_stats(&path);
                    if !stats.exists && !include_missing {
                        continue;
                    }
                    items.push(cache_item_from_path_spec(&spec, &path, stats));
                }
            }
            TargetKind::Command {
                executable,
                dry_args,
                dry_note,
                ..
            } => {
                if which::which(executable).is_ok() || include_missing {
                    let output = if which::which(executable).is_ok() {
                        let command_preview = format!("{} {}", executable, dry_args.join(" "));
                        Some(command_preview)
                    } else {
                        None
                    };
                    items.push(CacheItem {
                        id: spec.id.to_string(),
                        label: spec.label.to_string(),
                        category: spec.category.to_string(),
                        risk: spec.risk.clone(),
                        kind: "command".to_string(),
                        path: None,
                        command: output,
                        exists: which::which(executable).is_ok(),
                        selected_by_default: spec.default_selected,
                        size_bytes: 0,
                        file_count: 0,
                        dir_count: 0,
                        modified_rfc3339: None,
                        note: format!("{} Dry-run probe: {}.", spec.note, dry_note),
                        warning: if which::which(executable).is_ok() {
                            None
                        } else {
                            Some(format!("{} not found in PATH", executable))
                        },
                    });
                }
            }
        }
    }

    items.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.label.cmp(&b.label))
    });

    let total_bytes = items.iter().map(|i| i.size_bytes).sum();
    let mut category_totals = BTreeMap::new();
    for item in &items {
        *category_totals.entry(item.category.clone()).or_insert(0) += item.size_bytes;
    }

    Ok(DiscoverReport {
        generated_at: now_rfc3339(),
        total_bytes,
        items,
        category_totals,
        warnings,
    })
}

#[tauri::command]
fn dry_run(request: CleanRequest) -> Result<PlanReport, String> {
    plan_or_clean(request, true)
}

#[tauri::command]
fn clean(request: CleanRequest) -> Result<PlanReport, String> {
    plan_or_clean(request, false)
}

fn plan_or_clean(request: CleanRequest, dry: bool) -> Result<PlanReport, String> {
    let selected: HashSet<String> = request.selected_ids.iter().cloned().collect();
    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    let mut total_bytes = 0_u64;
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();

    for spec in specs() {
        if !selected.contains(spec.id) {
            continue;
        }

        if spec.risk == Risk::High && !request.include_high_risk {
            warnings.push(format!(
                "Skipped high-risk target '{}'. Enable high-risk cleanup explicitly.",
                spec.label
            ));
            continue;
        }

        match &spec.kind {
            TargetKind::Path { patterns } => {
                let expanded = expand_patterns(patterns, &mut warnings);
                if expanded.is_empty() {
                    actions.push(PlannedAction {
                        id: spec.id.to_string(),
                        label: spec.label.to_string(),
                        category: spec.category.to_string(),
                        risk: spec.risk.clone(),
                        kind: "path".to_string(),
                        path: None,
                        command: None,
                        size_bytes: 0,
                        action: if dry { "would remove" } else { "remove" }.to_string(),
                        status: "missing".to_string(),
                        output: None,
                    });
                    continue;
                }

                for path in expanded {
                    if !seen_paths.insert(path.clone()) {
                        continue;
                    }

                    let stats = path_stats(&path);
                    if !stats.exists {
                        actions.push(action_for_path(&spec, &path, stats, dry, "missing", None));
                        continue;
                    }

                    total_bytes += stats.size_bytes;

                    if dry {
                        actions.push(action_for_path(
                            &spec,
                            &path,
                            stats,
                            true,
                            "planned",
                            Some(if request.permanent {
                                "Would permanently delete this path.".to_string()
                            } else {
                                "Would move this path to Trash.".to_string()
                            }),
                        ));
                    } else {
                        let result = remove_path(&path, request.permanent);
                        match result {
                            Ok(status) => actions.push(action_for_path(
                                &spec,
                                &path,
                                stats,
                                false,
                                "cleaned",
                                Some(status),
                            )),
                            Err(err) => actions.push(action_for_path(
                                &spec,
                                &path,
                                stats,
                                false,
                                "error",
                                Some(err),
                            )),
                        }
                    }
                }
            }
            TargetKind::Command {
                executable,
                dry_args,
                clean_args,
                ..
            } => {
                if !request.allow_commands {
                    warnings.push(format!(
                        "Skipped command target '{}'. Enable command actions explicitly.",
                        spec.label
                    ));
                    continue;
                }

                let command_args = if dry { dry_args } else { clean_args };
                let command_string = format!("{} {}", executable, command_args.join(" "));
                if which::which(executable).is_err() {
                    actions.push(PlannedAction {
                        id: spec.id.to_string(),
                        label: spec.label.to_string(),
                        category: spec.category.to_string(),
                        risk: spec.risk.clone(),
                        kind: "command".to_string(),
                        path: None,
                        command: Some(command_string),
                        size_bytes: 0,
                        action: if dry { "would run" } else { "run" }.to_string(),
                        status: "missing executable".to_string(),
                        output: Some(format!("{} was not found in PATH", executable)),
                    });
                    continue;
                }

                if dry {
                    let output = run_command(executable, command_args);
                    actions.push(PlannedAction {
                        id: spec.id.to_string(),
                        label: spec.label.to_string(),
                        category: spec.category.to_string(),
                        risk: spec.risk.clone(),
                        kind: "command".to_string(),
                        path: None,
                        command: Some(command_string),
                        size_bytes: 0,
                        action: "would run".to_string(),
                        status: "dry-run output".to_string(),
                        output: Some(output.unwrap_or_else(|e| e)),
                    });
                } else {
                    let output = run_command(executable, command_args);
                    match output {
                        Ok(out) => actions.push(PlannedAction {
                            id: spec.id.to_string(),
                            label: spec.label.to_string(),
                            category: spec.category.to_string(),
                            risk: spec.risk.clone(),
                            kind: "command".to_string(),
                            path: None,
                            command: Some(command_string),
                            size_bytes: 0,
                            action: "ran".to_string(),
                            status: "done".to_string(),
                            output: Some(out),
                        }),
                        Err(err) => actions.push(PlannedAction {
                            id: spec.id.to_string(),
                            label: spec.label.to_string(),
                            category: spec.category.to_string(),
                            risk: spec.risk.clone(),
                            kind: "command".to_string(),
                            path: None,
                            command: Some(command_string),
                            size_bytes: 0,
                            action: "run".to_string(),
                            status: "error".to_string(),
                            output: Some(err),
                        }),
                    }
                }
            }
        }
    }

    Ok(PlanReport {
        generated_at: now_rfc3339(),
        dry_run: dry,
        permanent: request.permanent,
        total_bytes,
        actions,
        warnings,
    })
}

fn path_item_from_missing_spec(spec: &TargetSpec) -> CacheItem {
    CacheItem {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        category: spec.category.to_string(),
        risk: spec.risk.clone(),
        kind: "path".to_string(),
        path: None,
        command: None,
        exists: false,
        selected_by_default: false,
        size_bytes: 0,
        file_count: 0,
        dir_count: 0,
        modified_rfc3339: None,
        note: spec.note.to_string(),
        warning: Some("No matching path found".to_string()),
    }
}

fn cache_item_from_path_spec(spec: &TargetSpec, path: &Path, stats: PathStats) -> CacheItem {
    let warning = if spec.risk == Risk::High {
        Some("High risk: this may remove downloaded models, large stores, or important reusable state.".to_string())
    } else {
        None
    };

    CacheItem {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        category: spec.category.to_string(),
        risk: spec.risk.clone(),
        kind: "path".to_string(),
        path: Some(path.display().to_string()),
        command: None,
        exists: stats.exists,
        selected_by_default: spec.default_selected,
        size_bytes: stats.size_bytes,
        file_count: stats.file_count,
        dir_count: stats.dir_count,
        modified_rfc3339: stats.latest_modified.and_then(system_time_to_rfc3339),
        note: spec.note.to_string(),
        warning,
    }
}

fn action_for_path(
    spec: &TargetSpec,
    path: &Path,
    stats: PathStats,
    dry: bool,
    status: &str,
    output: Option<String>,
) -> PlannedAction {
    PlannedAction {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        category: spec.category.to_string(),
        risk: spec.risk.clone(),
        kind: "path".to_string(),
        path: Some(path.display().to_string()),
        command: None,
        size_bytes: stats.size_bytes,
        action: if dry { "would remove" } else { "remove" }.to_string(),
        status: status.to_string(),
        output,
    }
}

fn remove_path(path: &Path, permanent: bool) -> Result<String, String> {
    if !path.exists() {
        return Ok("Already missing".to_string());
    }

    if permanent {
        let metadata = fs::symlink_metadata(path).map_err(|e| format!("metadata error: {e}"))?;
        if metadata.is_dir() {
            fs::remove_dir_all(path).map_err(|e| format!("remove_dir_all error: {e}"))?;
        } else {
            fs::remove_file(path).map_err(|e| format!("remove_file error: {e}"))?;
        }
        Ok("Permanently deleted".to_string())
    } else {
        match trash::delete(path) {
            Ok(_) => Ok("Moved to Trash".to_string()),
            Err(e) => {
                let msg = e.to_string();
                // Finder is not running (SSH / headless session) — fall back to permanent delete.
                if msg.contains("isn't running") || msg.contains("Application isn't running") || msg.contains("-600") {
                    let metadata = fs::symlink_metadata(path).map_err(|e| format!("metadata error: {e}"))?;
                    if metadata.is_dir() {
                        fs::remove_dir_all(path).map_err(|e| format!("remove_dir_all error: {e}"))?;
                    } else {
                        fs::remove_file(path).map_err(|e| format!("remove_file error: {e}"))?;
                    }
                    Ok("Permanently deleted (Finder unavailable — headless session)".to_string())
                } else {
                    Err(format!("trash error: {msg}"))
                }
            }
        }
    }
}

fn expand_patterns(patterns: &[&'static str], warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for pattern in patterns {
        let expanded = expand_special_pattern(pattern, warnings);
        for p in expanded {
            let normalized = normalize_path(&p);
            if has_glob_chars(&normalized) {
                match glob(&normalized) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if seen.insert(entry.clone()) {
                                paths.push(entry);
                            }
                        }
                    }
                    Err(e) => warnings.push(format!("Invalid glob pattern '{}': {}", normalized, e)),
                }
            } else if seen.insert(PathBuf::from(&normalized)) {
                paths.push(PathBuf::from(normalized));
            }
        }
    }
    paths
}

fn expand_special_pattern(pattern: &str, warnings: &mut Vec<String>) -> Vec<String> {
    match pattern {
        "$BREW_CACHE" => {
            if let Ok(path) = command_stdout("brew", &["--cache"]) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    return vec![trimmed.to_string()];
                }
            }
            warnings.push("Could not resolve Homebrew cache via `brew --cache`; falling back to default macOS path if present.".to_string());
            Vec::new()
        }
        other => vec![other.to_string()],
    }
}

fn normalize_path(raw: &str) -> String {
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.display().to_string();
        }
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    raw.to_string()
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn path_stats(path: &Path) -> PathStats {
    path_stats_with_progress(path, |_| {})
}

fn path_stats_with_progress<F>(path: &Path, mut on_progress: F) -> PathStats
where
    F: FnMut(u64),
{
    if !path.exists() {
        return PathStats::default();
    }

    let mut stats = PathStats {
        exists: true,
        ..PathStats::default()
    };

    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return stats,
    };

    if metadata.is_file() {
        stats.size_bytes += metadata.len();
        stats.file_count += 1;
        stats.latest_modified = metadata.modified().ok();
        return stats;
    }

    if metadata.file_type().is_symlink() {
        stats.file_count += 1;
        stats.latest_modified = metadata.modified().ok();
        return stats;
    }

    for entry in WalkDir::new(path).follow_links(false).into_iter() {
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.metadata() else { continue };

        if meta.is_file() {
            stats.file_count += 1;
            stats.size_bytes = stats.size_bytes.saturating_add(meta.len());
            if stats.file_count % 1000 == 0 {
                on_progress(stats.file_count);
            }
        } else if meta.is_dir() {
            stats.dir_count += 1;
        }

        if let Ok(modified) = meta.modified() {
            match stats.latest_modified {
                Some(current) if modified > current => stats.latest_modified = Some(modified),
                None => stats.latest_modified = Some(modified),
                _ => {}
            }
        }
    }

    stats
}

fn run_command(executable: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run command: {e}"))?;

    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(format!(
            "exit status {}\n{}",
            output.status,
            text.trim()
        ))
    }
}

fn command_stdout(executable: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run command: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
fn memory_usage() -> Result<u64, String> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .map_err(|error| format!("failed to read process memory: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "ps exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let resident_kib = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .map_err(|error| format!("invalid process memory value: {error}"))?;
    Ok(resident_kib.saturating_mul(1024))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn system_time_to_rfc3339(t: SystemTime) -> Option<String> {
    let dt: DateTime<Utc> = t.into();
    Some(dt.to_rfc3339())
}

#[tauri::command]
fn list_path(path: String) -> Result<Vec<TreeNode>, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Ok(vec![]);
    }
    let meta = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return Ok(vec![TreeNode { name, path, is_dir: false, size_bytes: meta.len(), entry_count: 0 }]);
    }
    let mut nodes: Vec<TreeNode> = fs::read_dir(&p)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let child = entry.path();
            if meta.is_dir() {
                let stats = path_stats(&child);
                Some(TreeNode {
                    name,
                    path: child.display().to_string(),
                    is_dir: true,
                    size_bytes: stats.size_bytes,
                    entry_count: stats.file_count + stats.dir_count,
                })
            } else if meta.is_file() || meta.file_type().is_symlink() {
                Some(TreeNode { name, path: child.display().to_string(), is_dir: false, size_bytes: meta.len(), entry_count: 0 })
            } else {
                None
            }
        })
        .collect();
    nodes.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then(a.name.cmp(&b.name)));
    nodes.truncate(200);
    Ok(nodes)
}

// ── Project artifact scanner ──────────────────────────────────────────────────

/// Directories that are never project source roots — skip immediately.
fn artifact_scan_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | ".hg" | ".DS_Store" | ".Trash" | ".Spotlight-V100"
            | ".fseventsd" | ".DocumentRevisions-V100" | ".TemporaryItems"
            | "Library" | "System" | "Applications" | "Volumes" | "private"
            | "usr" | "bin" | "sbin" | "etc" | "var" | "tmp"
    )
}

fn detect_configured_artifact_kind(name: &str, path: &Path, rules: &[ArtifactRule]) -> Option<String> {
    let parent = path.parent().unwrap_or(path);
    rules.iter().find_map(|rule| {
        if !rule.enabled || rule.directory_name != name { return None; }
        if !rule.locations.is_empty()
            && !rule.locations.iter().any(|location| {
                location == "any" || (location == "android" && is_android_project_path(path))
            })
        {
            return None;
        }
        if rule.requires_outside.iter().any(|required| !parent.join(required).exists()) {
            return None;
        }
        if !rule.requires_outside_any.is_empty()
            && !rule.requires_outside_any.iter().any(|required| parent.join(required).exists())
        {
            return None;
        }
        if rule.requires_inside.iter().any(|required| !path.join(required).exists()) {
            return None;
        }
        Some(rule.artifact_kind.clone())
    })
}

/// Returns a human-readable label if `name` (at `path`) is a known artifact dir.
fn detect_artifact_kind(name: &str, path: &Path) -> Option<&'static str> {
    let p = path.parent().unwrap_or(path);
    match name {
        // Node / JS
        "node_modules" => Some("Node.js deps"),
        ".turbo"        => Some("Turborepo cache"),
        ".next"   if p.join("package.json").exists() => Some("Next.js build"),
        ".nuxt"   if p.join("package.json").exists() => Some("Nuxt build"),
        ".svelte-kit" if p.join("package.json").exists() => Some("SvelteKit build"),
        ".angular" if p.join("package.json").exists() => Some("Angular cache"),
        ".parcel-cache" => Some("Parcel cache"),
        // Python
        ".venv" | "venv" => Some("Python venv"),
        "__pycache__"    => Some("Python bytecode"),
        ".pytest_cache"  => Some("pytest cache"),
        ".mypy_cache"    => Some("mypy cache"),
        ".ruff_cache"    => Some("Ruff cache"),
        ".tox"           => Some("tox"),
        ".nox"           => Some("nox"),
        // Rust
        "target" if p.join("Cargo.toml").exists() => Some("Rust build"),
        // JVM
        "target" if p.join("pom.xml").exists()         => Some("Maven build"),
        "target" if p.join("build.gradle").exists()
                  || p.join("build.gradle.kts").exists() => Some("Gradle build"),
        ".gradle" if is_android_project_path(path) => Some("Android Gradle cache"),
        ".gradle" => Some("Gradle cache"),
        // Go
        "vendor" if p.join("go.mod").exists() => Some("Go vendor"),
        // PHP
        "vendor" if p.join("composer.json").exists() => Some("Composer vendor"),
        // iOS / macOS
        "Pods" if p.join("Podfile").exists() => Some("CocoaPods"),
        "DerivedData" => Some("Xcode DerivedData"),
        // Android project output and IDE/native build leftovers
        "build" if is_android_project_path(path) => Some("Android build"),
        ".cxx" if is_android_project_path(path) => Some("Android native build"),
        ".externalNativeBuild" if is_android_project_path(path) => Some("Android native build"),
        "captures" if is_android_project_path(path) => Some("Android captures"),
        ".navigation" if is_android_project_path(path) => Some("Android navigation cache"),
        ".kotlin" if is_android_project_path(path) => Some("Android Kotlin cache"),
        // Dart / Flutter
        ".dart_tool" => Some("Dart tool cache"),
        "build" if p.join("pubspec.yaml").exists() => Some("Flutter build"),
        // Infra
        ".terraform"        => Some("Terraform"),
        ".terragrunt-cache" => Some("Terragrunt"),
        ".pulumi"           => Some("Pulumi"),
        // Other
        ".stack-work" => Some("Haskell Stack"),
        "_build" if p.join("mix.exs").exists() => Some("Elixir build"),
        ".elixir_ls"  => Some("Elixir LS"),
        ".metals"     => Some("Scala Metals"),
        "elm-stuff" if p.join("elm.json").exists() => Some("Elm"),
        ".zig-cache"  => Some("Zig cache"),
        _ => None,
    }
}

/// Returns true when an artifact is inside a directory tree that looks like
/// an Android project or module. This keeps generic names such as `build` and
/// `captures` from matching unrelated projects.
fn is_android_project_path(path: &Path) -> bool {
    let mut current = path.parent();
    for _ in 0..=5 {
        let Some(dir) = current else { break };
        if dir.join("AndroidManifest.xml").exists()
            || dir.join("src/main/AndroidManifest.xml").exists()
            || (dir.join("settings.gradle").exists() || dir.join("settings.gradle.kts").exists())
                && dir.join("app").is_dir()
        {
            return true;
        }
        current = dir.parent();
    }
    false
}

#[tauri::command]
async fn scan_projects(
    app: AppHandle,
    options: ProjectScanOptions,
) -> Result<ProjectScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || scan_projects_impl(Some(&app), options))
        .await
        .map_err(|error| format!("Project scan worker failed: {error}"))?
}

fn scan_projects_impl(
    app: Option<&AppHandle>,
    options: ProjectScanOptions,
) -> Result<ProjectScanReport, String> {
    let root = PathBuf::from(normalize_path(&options.root));
    if !root.exists() {
        return Err(format!("Root directory not found: {}", root.display()));
    }

    let max_depth = options.max_depth.unwrap_or(8);
    let min_size = options.min_size_bytes.unwrap_or(0);
    let artifact_rules = options.artifact_rules.as_deref();
    let mut candidates: Vec<ProjectArtifact> = Vec::new();
    let mut directories_scanned = 0u64;

    emit_project_progress(
        app,
        ProjectProgress {
            phase: "scan",
            current: 0,
            total: None,
            path: root.display().to_string(),
            found: 0,
            bytes: 0,
        },
    );

    if let Some(saved_paths) = options.artifact_paths.as_ref() {
        let mut seen = HashSet::new();
        for raw_path in saved_paths {
            let path = PathBuf::from(normalize_path(raw_path));
            if !path.is_dir() || !seen.insert(path.clone()) { continue; }
            directories_scanned += 1;
            if let Some(candidate) = project_artifact_candidate(&path, artifact_rules) {
                candidates.push(candidate);
            }
        }
    } else {
        let mut it = WalkDir::new(&root)
            .min_depth(1)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter();

        loop {
            let entry = match it.next() {
                None          => break,
                Some(Err(_))  => continue,
                Some(Ok(e))   => e,
            };
            if !entry.file_type().is_dir() { continue; }

            directories_scanned += 1;

            let name = entry.file_name().to_string_lossy();

            if artifact_scan_skip(&name) {
                it.skip_current_dir();
                continue;
            }

            if let Some(candidate) = project_artifact_candidate(entry.path(), artifact_rules) {
                candidates.push(candidate);
                it.skip_current_dir(); // never recurse inside artifact dirs
            }

            if directories_scanned % 25 == 0 {
                emit_project_progress(
                    app,
                    ProjectProgress {
                        phase: "scan",
                        current: directories_scanned,
                        total: None,
                        path: entry.path().display().to_string(),
                        found: candidates.len() as u64,
                        bytes: 0,
                    },
                );
            }
        }
    }

    let candidate_count = candidates.len();
    emit_project_progress(
        app,
        ProjectProgress {
            phase: "scan-size",
            current: 0,
            total: Some(candidate_count as u64),
            path: root.display().to_string(),
            found: 0,
            bytes: 0,
        },
    );

    let mut indexed_artifacts = Vec::new();
    let mut emitted_artifact_count = 0usize;
    let mut measured_count = 0u64;
    let mut measured_bytes = 0u64;

    if candidate_count > 0 {
        let available_workers = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(2);
        let worker_count = available_workers.min(4).min(candidate_count).max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .thread_name(|index| format!("artifact-sizer-{index}"))
            .build()
            .map_err(|error| format!("Could not start artifact sizing workers: {error}"))?;
        let (sender, receiver) = mpsc::channel();

        std::thread::scope(|scope| -> Result<(), String> {
            let producer = scope.spawn(move || {
                pool.install(|| {
                    candidates.into_par_iter().enumerate().for_each_with(
                        sender,
                        |sender, (index, mut artifact)| {
                            let progress_path = artifact.path.clone();
                            let stats = path_stats_with_progress(
                                Path::new(&artifact.path),
                                |files_scanned| {
                                    let _ = sender.send(ArtifactSizingEvent::Progress {
                                        path: progress_path.clone(),
                                        files_scanned,
                                    });
                                },
                            );
                            artifact.size_bytes = stats.size_bytes;
                            artifact.file_count = stats.file_count;
                            let artifact = (stats.size_bytes >= min_size).then_some(artifact);
                            let _ = sender.send(ArtifactSizingEvent::Complete { index, artifact });
                        },
                    );
                });
            });

            for event in receiver {
                match event {
                    ArtifactSizingEvent::Progress { path, files_scanned } => {
                        emit_project_progress(
                            app,
                            ProjectProgress {
                                phase: "scan-size",
                                current: measured_count,
                                total: Some(candidate_count as u64),
                                path: format!("{path} ({files_scanned} files measured)"),
                                found: indexed_artifacts.len() as u64,
                                bytes: measured_bytes,
                            },
                        );
                    }
                    ArtifactSizingEvent::Complete { index, artifact } => {
                        measured_count += 1;
                        if let Some(artifact) = artifact {
                            measured_bytes = measured_bytes.saturating_add(artifact.size_bytes);
                            indexed_artifacts.push((index, artifact));
                        }

                        if indexed_artifacts.len() - emitted_artifact_count >= 25 {
                            let batch: Vec<ProjectArtifact> = indexed_artifacts
                                [emitted_artifact_count..]
                                .iter()
                                .map(|(_, artifact)| artifact.clone())
                                .collect();
                            emit_project_artifact_batch(app, &root, &batch);
                            emitted_artifact_count = indexed_artifacts.len();
                        }

                        emit_project_progress(
                            app,
                            ProjectProgress {
                                phase: "scan-size",
                                current: measured_count,
                                total: Some(candidate_count as u64),
                                path: root.display().to_string(),
                                found: indexed_artifacts.len() as u64,
                                bytes: measured_bytes,
                            },
                        );
                    }
                }
            }

            producer
                .join()
                .map_err(|_| "Artifact sizing worker panicked".to_string())?;
            Ok(())
        })?;
    }

    if emitted_artifact_count < indexed_artifacts.len() {
        let batch: Vec<ProjectArtifact> = indexed_artifacts[emitted_artifact_count..]
            .iter()
            .map(|(_, artifact)| artifact.clone())
            .collect();
        emit_project_artifact_batch(app, &root, &batch);
    }

    indexed_artifacts.sort_by_key(|(index, _)| *index);
    let artifacts: Vec<ProjectArtifact> = indexed_artifacts
        .into_iter()
        .map(|(_, artifact)| artifact)
        .collect();

    let total_bytes     = artifacts.iter().map(|a| a.size_bytes).sum();
    let artifact_count  = artifacts.len() as u64;
    let project_count   = artifacts.iter()
        .map(|a| a.project_path.as_str())
        .collect::<HashSet<_>>()
        .len() as u64;

    emit_project_progress(
        app,
        ProjectProgress {
            phase: "scan-complete",
            current: directories_scanned,
            total: Some(directories_scanned),
            path: root.display().to_string(),
            found: artifact_count,
            bytes: total_bytes,
        },
    );

    Ok(ProjectScanReport {
        root: root.display().to_string(),
        artifacts,
        total_bytes,
        artifact_count,
        project_count,
        generated_at: chrono::Local::now().to_rfc3339(),
    })
}

fn project_artifact_candidate(path: &Path, rules: Option<&[ArtifactRule]>) -> Option<ProjectArtifact> {
    if !path.is_dir() { return None; }
    let name = path.file_name()?.to_string_lossy();
    let kind = if let Some(rules) = rules {
        detect_configured_artifact_kind(&name, path, rules)?
    } else {
        detect_artifact_kind(&name, path)?.to_string()
    };
    let parent = path.parent().unwrap_or(path);
    let modified_secs = fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Some(ProjectArtifact {
        path: path.display().to_string(),
        dir_name: name.to_string(),
        artifact_kind: kind,
        project_name: parent.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        project_path: parent.display().to_string(),
        size_bytes: 0,
        file_count: 0,
        modified_secs,
    })
}

#[tauri::command]
async fn delete_artifacts(app: AppHandle, request: DeleteArtifactsRequest) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || delete_artifacts_impl(Some(&app), request))
        .await
        .map_err(|error| format!("Artifact deletion worker failed: {error}"))?
}

fn delete_artifacts_impl(
    app: Option<&AppHandle>,
    request: DeleteArtifactsRequest,
) -> Result<u64, String> {
    let mut freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let total = request.paths.len() as u64;

    emit_project_progress(
        app,
        ProjectProgress {
            phase: "delete",
            current: 0,
            total: Some(total),
            path: String::new(),
            found: 0,
            bytes: 0,
        },
    );

    for (index, path_str) in request.paths.iter().enumerate() {
        let p = PathBuf::from(path_str);
        if !p.exists() {
            emit_project_progress(
                app,
                ProjectProgress {
                    phase: "delete",
                    current: (index + 1) as u64,
                    total: Some(total),
                    path: path_str.clone(),
                    found: 0,
                    bytes: freed,
                },
            );
            continue;
        }

        let path_for_progress = path_str.clone();
        let size = path_stats_with_progress(&p, |files_scanned| {
            emit_project_progress(
                app,
                ProjectProgress {
                    phase: "delete-size",
                    current: index as u64,
                    total: Some(total),
                    path: format!("{} ({} files measured)", path_for_progress, files_scanned),
                    found: errors.len() as u64,
                    bytes: freed,
                },
            );
        })
        .size_bytes;

        let result = if request.permanent {
            fs::remove_dir_all(&p).map_err(|e| e.to_string())
        } else {
            trash::delete(&p).map_err(|e| e.to_string())
        };

        match result {
            Ok(_)    => freed += size,
            Err(e)   => errors.push(format!("{}: {}", path_str, e)),
        }

        emit_project_progress(
            app,
            ProjectProgress {
                phase: "delete",
                current: (index + 1) as u64,
                total: Some(total),
                path: path_str.clone(),
                found: errors.len() as u64,
                bytes: freed,
            },
        );
    }

    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(freed)
}

// ── Public API (used by the CLI binary) ──────────────────────────────────────

pub fn run_discover(options: DiscoverOptions) -> Result<DiscoverReport, String> {
    discover(options)
}

pub fn run_dry_run(request: CleanRequest) -> Result<PlanReport, String> {
    plan_or_clean(request, true)
}

pub fn run_clean(request: CleanRequest) -> Result<PlanReport, String> {
    plan_or_clean(request, false)
}

pub fn run_list_path(path: String) -> Result<Vec<TreeNode>, String> {
    list_path(path)
}

pub fn run_scan_projects(options: ProjectScanOptions) -> Result<ProjectScanReport, String> {
    scan_projects_impl(None, options)
}

// ── Tauri entry point ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![discover, dry_run, clean, list_path, scan_projects, delete_artifacts, memory_usage])
        .run(tauri::generate_context!())
        .expect("error while running macOS Dev Cache Cleaner");
}
