use clap::{Parser, Subcommand, ValueEnum};
use macos_dev_cache_cleaner_lib::{
    run_clean, run_discover, run_dry_run, CacheItem, CleanRequest, DiscoverOptions, DiscoverReport,
    PlanReport, Risk,
};
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;

// ── permission helpers ────────────────────────────────────────────────────────

fn is_permission_error(e: &str) -> bool {
    e.contains("Permission denied")
        || e.contains("Operation not permitted")
        || e.contains("(os error 1)")
        || e.contains("(os error 13)")
}

fn permission_hint() {
    eprintln!();
    eprintln!("Some paths were blocked by macOS. cache-cleaner needs Full Disk Access.");
    eprintln!("  Run:  cache-cleaner permissions --open");
    eprintln!("  Or:   System Settings > Privacy & Security > Full Disk Access");
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "cache-cleaner",
    version,
    about = "Developer cache cleaner — server/CLI edition",
    long_about = "Discover, preview, and clean developer caches without a GUI.\n\
                  Defaults to `scan` when no subcommand is given."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    #[arg(
        long,
        global = true,
        help = "Output as JSON instead of human-readable text"
    )]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan and display all found caches (default)
    Scan {
        #[arg(long, help = "Include targets whose paths do not exist on disk")]
        include_missing: bool,
    },
    /// List every known cache target with its ID and metadata
    List {
        #[arg(long, help = "Filter by category name (case-insensitive substring)")]
        category: Option<String>,
        #[arg(long, value_enum, help = "Filter by risk level")]
        risk: Option<RiskArg>,
    },
    /// Preview what would be cleaned without touching anything
    #[command(name = "dry-run")]
    DryRun {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        opts: CleanOpts,
    },
    /// Check and fix macOS privacy permissions (Full Disk Access)
    Permissions {
        #[arg(
            long,
            help = "Open System Settings > Privacy & Security > Full Disk Access"
        )]
        open: bool,
    },
    /// Manage the automated LaunchAgent service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Clean the selected caches
    Clean {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        opts: CleanOpts,
        #[arg(
            long,
            short = 'y',
            help = "Skip the confirmation prompt (useful for scripts and automation)"
        )]
        yes: bool,
    },
}

#[derive(clap::Args)]
struct Selection {
    #[arg(
        long,
        value_delimiter = ',',
        help = "Comma-separated target IDs (from `list`). \
                Repeat the flag or separate with commas: --ids npm-cache,pip-cache"
    )]
    ids: Option<Vec<String>>,
    #[arg(
        long,
        help = "Select default-safe targets (used when no other selection flag is given)"
    )]
    defaults: bool,
    #[arg(long, help = "Select every known target")]
    all: bool,
    #[arg(long, help = "Select low-risk targets only")]
    low_risk: bool,
    #[arg(
        long,
        help = "Further restrict the selection to a category (case-insensitive substring)"
    )]
    category: Option<String>,
}

#[derive(clap::Args)]
struct CleanOpts {
    #[arg(long, help = "Permanently delete instead of moving to Trash")]
    permanent: bool,
    #[arg(
        long,
        help = "Allow external command targets (brew, docker, pnpm, go …)"
    )]
    allow_commands: bool,
    #[arg(
        long,
        help = "Include high-risk targets (model stores, simulator devices, device backups …)"
    )]
    include_high_risk: bool,
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install and activate the LaunchAgent (creates ~/Library/LaunchAgents plist)
    Install {
        #[arg(
            long,
            short = 's',
            value_enum,
            default_value = "weekly",
            help = "How often to run"
        )]
        schedule: ScheduleArg,
        #[arg(
            long,
            default_value = "3",
            help = "Hour of day to run (0–23, local time)"
        )]
        hour: u8,
        #[arg(
            long,
            help = "Path to the cache-cleaner binary [default: current executable]"
        )]
        bin: Option<PathBuf>,
        #[arg(long, help = "Permanently delete instead of moving to Trash")]
        permanent: bool,
        #[arg(long, help = "Allow command targets (brew, docker, pnpm …)")]
        allow_commands: bool,
    },
    /// Remove the LaunchAgent plist and stop the service
    Uninstall,
    /// Show service status and last log lines
    Status,
}

#[derive(ValueEnum, Clone, Copy)]
enum ScheduleArg {
    Daily,
    Weekly,
}

#[derive(ValueEnum, Clone)]
enum RiskArg {
    Low,
    Medium,
    High,
}

// ── dispatch ──────────────────────────────────────────────────────────────────

fn run(cli: Cli) -> Result<(), String> {
    match cli.command.unwrap_or(Cmd::Scan {
        include_missing: false,
    }) {
        Cmd::Scan { include_missing } => cmd_scan(include_missing, cli.json),
        Cmd::List { category, risk } => cmd_list(category, risk, cli.json),
        Cmd::DryRun { selection, opts } => cmd_dry_run(selection, opts, cli.json),
        Cmd::Clean {
            selection,
            opts,
            yes,
        } => cmd_clean(selection, opts, yes, cli.json),
        Cmd::Permissions { open } => cmd_permissions(open),
        Cmd::Service { action } => cmd_service(action),
    }
}

// ── subcommands ───────────────────────────────────────────────────────────────

fn cmd_scan(include_missing: bool, json: bool) -> Result<(), String> {
    eprintln!("Scanning...");
    let report = run_discover(DiscoverOptions {
        include_missing: Some(include_missing),
    })?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    let has_perm_issues = report.warnings.iter().any(|w| is_permission_error(w));
    print_scan(&report);
    if has_perm_issues {
        permission_hint();
    }
    Ok(())
}

fn cmd_list(category: Option<String>, risk: Option<RiskArg>, json: bool) -> Result<(), String> {
    let report = run_discover(DiscoverOptions {
        include_missing: Some(true),
    })?;

    // Deduplicate by id so glob-expanded targets (one per IDE install) appear once.
    let mut seen_ids: HashSet<&str> = HashSet::new();
    let items: Vec<&CacheItem> = report
        .items
        .iter()
        .filter(|i| {
            let cat_ok = category.as_ref().map_or(true, |c| {
                i.category.to_lowercase().contains(&c.to_lowercase())
            });
            let risk_ok = risk.as_ref().map_or(true, |r| match r {
                RiskArg::Low => i.risk == Risk::Low,
                RiskArg::Medium => i.risk == Risk::Medium,
                RiskArg::High => i.risk == Risk::High,
            });
            cat_ok && risk_ok && seen_ids.insert(i.id.as_str())
        })
        .collect();

    if json {
        let out: Vec<_> = items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "id": i.id,
                    "label": i.label,
                    "category": i.category,
                    "risk": risk_str(&i.risk),
                    "selectedByDefault": i.selected_by_default,
                    "note": i.note,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    println!(
        "{:<36} {:<26} {:<8} {:<8} {}",
        "ID", "Category", "Risk", "Default", "Label"
    );
    println!("{}", "─".repeat(108));
    for i in &items {
        let def = if i.selected_by_default { "yes" } else { "" };
        println!(
            "{:<36} {:<26} {:<8} {:<8} {}",
            i.id,
            i.category,
            risk_str(&i.risk),
            def,
            i.label
        );
    }
    println!("\n{} targets listed", items.len());
    Ok(())
}

fn cmd_dry_run(sel: Selection, opts: CleanOpts, json: bool) -> Result<(), String> {
    let ids = resolve_ids(&sel)?;
    eprintln!("Dry-running {} target(s)...", ids.len());
    let plan = run_dry_run(build_request(ids, &opts))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    print_plan(&plan);
    Ok(())
}

fn cmd_clean(sel: Selection, opts: CleanOpts, yes: bool, json: bool) -> Result<(), String> {
    let ids = resolve_ids(&sel)?;

    if !yes {
        let word = if opts.permanent { "DELETE" } else { "CLEAN" };
        eprint!(
            "This will {} {} target(s). Type {} to confirm: ",
            if opts.permanent {
                "permanently delete"
            } else {
                "move to Trash"
            },
            ids.len(),
            word
        );
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?;
        if input.trim() != word {
            eprintln!("Cancelled.");
            return Ok(());
        }
    }

    eprintln!("Cleaning {} target(s)...", ids.len());
    let plan = run_clean(build_request(ids, &opts))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    let has_perm_errors = plan
        .actions
        .iter()
        .any(|a| a.status == "error" && a.output.as_deref().map_or(false, is_permission_error));
    print_plan(&plan);
    if has_perm_errors {
        permission_hint();
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn resolve_ids(sel: &Selection) -> Result<Vec<String>, String> {
    if let Some(ids) = &sel.ids {
        if !ids.is_empty() {
            return Ok(ids.clone());
        }
    }

    let report = run_discover(DiscoverOptions {
        include_missing: Some(false),
    })?;

    let mut ids: Vec<String> = if sel.all {
        report.items.iter().map(|i| i.id.clone()).collect()
    } else if sel.low_risk {
        report
            .items
            .iter()
            .filter(|i| i.risk == Risk::Low)
            .map(|i| i.id.clone())
            .collect()
    } else {
        // default: selectedByDefault (also applies when --defaults is passed)
        report
            .items
            .iter()
            .filter(|i| i.selected_by_default)
            .map(|i| i.id.clone())
            .collect()
    };

    if let Some(cat) = &sel.category {
        let cat_lower = cat.to_lowercase();
        let allowed: HashSet<String> = report
            .items
            .iter()
            .filter(|i| i.category.to_lowercase().contains(&cat_lower))
            .map(|i| i.id.clone())
            .collect();
        ids.retain(|id| allowed.contains(id));
    }

    if ids.is_empty() {
        return Err(
            "No targets selected. Use --defaults, --all, --low-risk, or --ids <id,...>.".into(),
        );
    }

    Ok(ids)
}

fn build_request(selected_ids: Vec<String>, opts: &CleanOpts) -> CleanRequest {
    CleanRequest {
        selected_ids,
        permanent: opts.permanent,
        allow_commands: opts.allow_commands,
        include_high_risk: opts.include_high_risk,
    }
}

// ── permissions ───────────────────────────────────────────────────────────────

/// Directories that require Full Disk Access on macOS.
/// Each entry is (display_name, path_relative_to_home).
const FDA_PROBES: &[(&str, &str)] = &[
    ("Library/Application Support", "Library/Application Support"),
    ("Library/Containers", "Library/Containers"),
    ("Library/Caches", "Library/Caches"),
    ("Library/Logs", "Library/Logs"),
    ("Library/Developer", "Library/Developer"),
];

fn cmd_permissions(open_settings: bool) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Full Disk Access settings apply only to macOS. On Windows, close running developer tools and check folder permissions.".into());
    }
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;

    println!("macOS Full Disk Access — permission check");
    println!("{}", "─".repeat(52));

    let mut blocked = 0usize;

    for (label, rel) in FDA_PROBES {
        let path = home.join(rel);
        let status = match std::fs::read_dir(&path) {
            Ok(_) => "\x1b[32mok\x1b[0m    ",
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                blocked += 1;
                "\x1b[31mblocked\x1b[0m"
            }
            Err(_) => "\x1b[33mmissing\x1b[0m",
        };
        println!("  [{}]  ~/{}", status, label);
    }

    println!("{}", "─".repeat(52));

    if blocked == 0 {
        println!("All probed paths are accessible. No action needed.");
        if open_settings {
            open_fda_settings();
        }
        return Ok(());
    }

    println!("{} path(s) are blocked by macOS privacy controls.", blocked);
    println!();
    println!("To fix:  grant cache-cleaner Full Disk Access in");
    println!("         System Settings > Privacy & Security > Full Disk Access");
    println!("         then add this binary:");
    if let Ok(exe) = std::env::current_exe() {
        println!("           {}", exe.display());
    }

    if open_settings {
        open_fda_settings();
    } else {
        let is_remote = std::env::var("SSH_TTY").is_ok() || std::env::var("SSH_CONNECTION").is_ok();
        println!();
        if is_remote {
            println!("Remote session detected. Use sudo to bypass Full Disk Access:");
            println!("  sudo cache-cleaner scan");
            println!("  sudo cache-cleaner clean --defaults");
        } else {
            println!("Tip:  run  cache-cleaner permissions --open  to jump there directly.");
        }
    }

    Ok(())
}

fn open_fda_settings() {
    // SSH / headless sessions have no display; opening a GUI pane makes no sense.
    if std::env::var("SSH_TTY").is_ok() || std::env::var("SSH_CONNECTION").is_ok() {
        eprintln!("Remote session detected — cannot open System Settings.");
        eprintln!("On a remote machine, run with sudo to bypass Full Disk Access:");
        eprintln!("  sudo cache-cleaner scan");
        return;
    }
    println!();
    println!("Opening System Settings > Privacy & Security > Full Disk Access …");
    let status = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .status();
    if status.map_or(true, |s| !s.success()) {
        eprintln!("Could not open System Settings automatically.");
        eprintln!("Navigate manually: System Settings > Privacy & Security > Full Disk Access");
    }
}

// ── service management ────────────────────────────────────────────────────────

const PLIST_LABEL: &str = "com.macos-dev-cache-cleaner.autorun";
const PLIST_FILE: &str = "com.macos-dev-cache-cleaner.plist";
const LOG_FILE: &str = "dev-cache-cleaner-autorun.log";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap()
        .join("Library/LaunchAgents")
        .join(PLIST_FILE)
}
fn log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap()
        .join("Library/Logs")
        .join(LOG_FILE)
}

fn cmd_service(action: ServiceAction) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("LaunchAgent services are macOS-only. On Windows, schedule cache-cleaner.exe with Task Scheduler.".into());
    }
    match action {
        ServiceAction::Install {
            schedule,
            hour,
            bin,
            permanent,
            allow_commands,
        } => svc_install(schedule, hour, bin, permanent, allow_commands),
        ServiceAction::Uninstall => svc_uninstall(),
        ServiceAction::Status => svc_status(),
    }
}

fn svc_install(
    schedule: ScheduleArg,
    hour: u8,
    bin: Option<PathBuf>,
    permanent: bool,
    allow_commands: bool,
) -> Result<(), String> {
    if hour > 23 {
        return Err("--hour must be 0–23".into());
    }

    let bin_path = bin
        .map(|p| p.canonicalize().unwrap_or(p))
        .or_else(|| std::env::current_exe().ok())
        .ok_or("Cannot determine binary path; pass --bin <path>")?;

    if !bin_path.exists() {
        return Err(format!("Binary not found: {}", bin_path.display()));
    }

    let plist = plist_path();
    let log = log_path();

    // Warn if binary is inside a build dir (not a permanent install location)
    let bin_str = bin_path.to_string_lossy();
    if bin_str.contains("/target/debug/") || bin_str.contains("/target/release/") {
        eprintln!("Warning: binary is inside a Cargo build directory.");
        eprintln!("         It will break if the project is cleaned. Consider copying it to");
        eprintln!("         /usr/local/bin or ~/.local/bin and re-running with --bin <path>.");
    }

    let mut args: Vec<&str> = vec!["clean", "--defaults", "--yes"];
    if permanent {
        args.push("--permanent");
    }
    if allow_commands {
        args.push("--allow-commands");
    }

    let args_xml: String = args
        .iter()
        .map(|a| format!("        <string>{}</string>", xml_escape(a)))
        .collect::<Vec<_>>()
        .join("\n");

    let calendar = match schedule {
        ScheduleArg::Daily  => format!(
            "    <dict>\n        <key>Hour</key>\n        <integer>{hour}</integer>\n        <key>Minute</key>\n        <integer>0</integer>\n    </dict>"
        ),
        ScheduleArg::Weekly => format!(
            "    <dict>\n        <key>Weekday</key>\n        <integer>0</integer>\n        <key>Hour</key>\n        <integer>{hour}</integer>\n        <key>Minute</key>\n        <integer>0</integer>\n    </dict>"
        ),
    };

    let plist_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
{args}
    </array>
    <key>StartCalendarInterval</key>
{calendar}
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>
"#,
        label = PLIST_LABEL,
        bin = xml_escape(&bin_path.to_string_lossy()),
        args = args_xml,
        calendar = calendar,
        log = xml_escape(&log.to_string_lossy()),
    );

    // Unload existing service before overwriting
    if plist.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w", &plist.to_string_lossy()])
            .output();
        println!("Replaced existing service.");
    }

    std::fs::write(&plist, &plist_xml).map_err(|e| format!("Failed to write plist: {e}"))?;

    let out = std::process::Command::new("launchctl")
        .args(["load", "-w", &plist.to_string_lossy()])
        .output()
        .map_err(|e| format!("Failed to run launchctl: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("launchctl load failed: {err}"));
    }

    let sched_str = match schedule {
        ScheduleArg::Daily => format!("daily at {:02}:00", hour),
        ScheduleArg::Weekly => format!("weekly (Sundays) at {:02}:00", hour),
    };

    println!("Service installed and activated.");
    println!("  Schedule : {}", sched_str);
    println!("  Binary   : {}", bin_path.display());
    println!("  Plist    : {}", plist.display());
    println!("  Log      : {}", log.display());
    println!(
        "  Mode     : {}",
        if permanent {
            "permanent delete"
        } else {
            "move to Trash"
        }
    );
    println!();
    println!("To stop:   cache-cleaner service uninstall");
    println!("To check:  cache-cleaner service status");
    Ok(())
}

fn svc_uninstall() -> Result<(), String> {
    let plist = plist_path();
    if !plist.exists() {
        println!("Service is not installed.");
        return Ok(());
    }
    let out = std::process::Command::new("launchctl")
        .args(["unload", "-w", &plist.to_string_lossy()])
        .output()
        .map_err(|e| format!("Failed to run launchctl: {e}"))?;
    if !out.status.success() {
        eprintln!(
            "Warning: launchctl unload: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    std::fs::remove_file(&plist).map_err(|e| format!("Failed to remove plist: {e}"))?;
    println!("Service removed: {}", plist.display());
    Ok(())
}

fn svc_status() -> Result<(), String> {
    let plist = plist_path();
    if !plist.exists() {
        println!("Status : not installed");
        println!("  Run `cache-cleaner service install` to set up automated cleaning.");
        return Ok(());
    }
    println!("Plist  : {}", plist.display());

    let out = std::process::Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()
        .map_err(|e| format!("Failed to run launchctl: {e}"))?;

    if out.status.success() {
        println!("Status : loaded (active)");
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let t = line.trim().trim_matches('"').trim_matches(';');
            if t.starts_with("PID") || t.starts_with("LastExitStatus") {
                println!("         {}", t);
            }
        }
    } else {
        println!("Status : installed but not loaded");
        println!("  Activate with: launchctl load -w {}", plist.display());
    }

    let log = log_path();
    if log.exists() {
        println!("Log    : {}", log.display());
        if let Ok(content) = std::fs::read_to_string(&log) {
            let lines: Vec<&str> = content.lines().collect();
            let tail = &lines[lines.len().saturating_sub(12)..];
            if !tail.is_empty() {
                println!("\nLast log entries:");
                for l in tail {
                    println!("  {}", l);
                }
            }
        }
    } else {
        println!("Log    : (no log yet — service hasn't run)");
    }
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── output ────────────────────────────────────────────────────────────────────

fn print_scan(report: &DiscoverReport) {
    const W: usize = 112;
    println!("macOS Dev Cache Cleaner — scan results");
    println!("{}", "─".repeat(W));

    let mut sorted = report.items.clone();
    sorted.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(b.size_bytes.cmp(&a.size_bytes))
    });

    let mut last_cat = String::new();
    for item in &sorted {
        if item.category != last_cat {
            println!("\n  {}", item.category.to_uppercase());
            last_cat = item.category.clone();
        }

        let location = item
            .path
            .as_deref()
            .or(item.command.as_deref())
            .unwrap_or("—");
        let loc = if location.len() > 50 {
            format!("…{}", &location[location.len() - 49..])
        } else {
            location.to_string()
        };

        let files_str = if item.file_count > 0 {
            format!("{} files", fmt_num(item.file_count))
        } else {
            String::new()
        };

        println!(
            "  [{:<6}] {:<44} {:>9}  {:>13}  {}",
            risk_str(&item.risk),
            trunc(&item.label, 44),
            fmt_bytes(item.size_bytes),
            files_str,
            loc,
        );

        if let Some(w) = &item.warning {
            println!("           ⚠  {}", w);
        }
    }

    println!("\n{}", "─".repeat(W));
    println!(
        "Total reclaimable: {}  ({} targets found)",
        fmt_bytes(report.total_bytes),
        report.items.len()
    );

    let mut cats: Vec<(&String, &u64)> = report
        .category_totals
        .iter()
        .filter(|(_, &v)| v > 0)
        .collect();
    if !cats.is_empty() {
        cats.sort_by(|a, b| b.1.cmp(a.1));
        println!("\nBy category:");
        for (cat, bytes) in cats {
            println!("  {:<32} {}", cat, fmt_bytes(*bytes));
        }
    }

    if !report.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &report.warnings {
            println!("  !  {}", w);
        }
    }
}

fn print_plan(plan: &PlanReport) {
    const W: usize = 84;
    let mode = if plan.dry_run { "DRY RUN" } else { "CLEANUP" };
    let del = if plan.permanent {
        "permanent delete"
    } else {
        "move to Trash"
    };
    println!("{} — {} — {}", mode, del, plan.generated_at);
    println!("{}", "─".repeat(W));

    for a in &plan.actions {
        println!(
            "[{:<12}] {} ({}, {})",
            a.status,
            a.label,
            risk_str(&a.risk),
            a.category
        );
        if let Some(p) = &a.path {
            println!("               path: {}", p);
        }
        if let Some(c) = &a.command {
            println!("            command: {}", c);
        }
        if a.size_bytes > 0 {
            println!("               size: {}", fmt_bytes(a.size_bytes));
        }
        if let Some(out) = &a.output {
            let snippet = if out.len() > 600 {
                format!("{}…", &out[..600])
            } else {
                out.clone()
            };
            for (i, line) in snippet.lines().enumerate() {
                if i == 0 {
                    println!("             output: {}", line);
                } else {
                    println!("                     {}", line);
                }
            }
        }
    }

    println!("{}", "─".repeat(W));
    println!("Affected: {}", fmt_bytes(plan.total_bytes));

    if !plan.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &plan.warnings {
            println!("  !  {}", w);
        }
    }
}

fn risk_str(r: &Risk) -> &'static str {
    match r {
        Risk::Low => "low",
        Risk::Medium => "medium",
        Risk::High => "HIGH",
    }
}

fn fmt_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "—".to_string();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0usize;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", bytes)
    } else if v >= 10.0 {
        format!("{:.0} {}", v, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
