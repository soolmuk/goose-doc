use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use tracing::info;

use goose_doc::addr;
use goose_doc::cli::{Cli, Command, ServiceAction};
use goose_doc::config;
use goose_doc::docs::{self, DocsRoot};
use goose_doc::panel::Panel;
use goose_doc::server;
use goose_doc::service;
use goose_doc::settings::Settings;

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();

    match &cli.command {
        Some(
            Command::Fetch { .. }
            | Command::Doctor { .. }
            | Command::Addresses
            | Command::Service { .. },
        ) => runtime()?.block_on(run_command(cli)),
        // --headless never touches the windowing system, so it works anywhere.
        None if cli.headless => runtime()?.block_on(serve_headless(&cli)),
        None => serve_with_panel(cli),
    }
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the async runtime")
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_env("GOOSE_DOC_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn cache_root(cli: &Cli) -> PathBuf {
    cli.cache_dir.clone().unwrap_or_else(config::cache_dir)
}

async fn run_command(cli: Cli) -> Result<()> {
    let cache = cache_root(&cli);

    match &cli.command {
        Some(Command::Fetch {
            version,
            base_url,
            cache_dir,
            token,
            variant,
        }) => {
            let cache = cache_dir.clone().unwrap_or(cache);
            let source = docs::BundleSource::new(base_url, token.clone(), (*variant).into());
            let root = docs::fetch(&cache, version, &source).await?;
            println!(
                "{} ({} pages)\n{}",
                root.version.as_deref().unwrap_or(version),
                root.entries,
                root.path.display()
            );
        }
        Some(Command::Doctor {
            docs_dir,
            docs_version,
            cache_dir,
        }) => {
            let cache = cache_dir.clone().unwrap_or(cache);
            let root = resolve_docs(docs_dir.as_deref(), docs_version.as_deref(), &cache)?;
            report(&root);
        }
        Some(Command::Addresses) => report_addresses(),
        Some(Command::Service {
            action,
            apply,
            start,
            docs_dir,
            docs_version,
            cache_dir,
        }) => {
            let cache = cache_dir.clone().unwrap_or(cache);
            run_service(
                *action,
                *apply,
                *start,
                docs_dir.as_deref(),
                docs_version.as_deref(),
                &cache,
                &cli,
            )?;
        }
        None => unreachable!("serve paths are handled in main"),
    }

    Ok(())
}

async fn serve_headless(cli: &Cli) -> Result<()> {
    let root = resolve_docs(
        cli.docs_dir.as_deref(),
        cli.docs_version.as_deref(),
        &cache_root(cli),
    )?;

    let mut running = server::start(&root, &cli.bind_addr(), cli.port).await?;
    print_started(&running);

    if cli.open {
        let _ = open::that(&running.url);
    }

    info!("running headless, press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;
    info!("stopping");
    running.stop();

    Ok(())
}

fn serve_with_panel(cli: Cli) -> Result<()> {
    let path = settings_path();
    let settings = merge_settings(&cli, Settings::load_or_default(&path));

    // The panel opens a window on the main thread, and it starts the server by
    // blocking on the current runtime handle, so the runtime must be entered
    // for the whole panel session.
    let runtime = runtime()?;
    let _guard = runtime.enter();

    let panel = Panel::new(settings, path, cache_root(&cli));

    // A server has no windowing system, and a CI runner may not either. Rather
    // than fail, report the reason once and serve without the panel.
    match run_panel(panel) {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Could not open the panel ({error}).");
            eprintln!("Serving headless instead. Pass --headless to silence this.");
            runtime.block_on(serve_headless(&cli))
        }
    }
}

/// Command-line values win over saved ones, so a one-off invocation is not
/// silently overridden by the previous session.
fn merge_settings(cli: &Cli, mut settings: Settings) -> Settings {
    if let Some(bind) = &cli.bind {
        settings.bind = bind.clone();
    }
    if cli.local_only {
        settings.bind = addr::LOOPBACK.to_string();
    }
    if cli.port != config::DEFAULT_PORT {
        settings.port = cli.port;
    }
    if let Some(dir) = &cli.docs_dir {
        settings.docs_dir = Some(dir.clone());
        settings.docs_version = None;
    }
    if let Some(version) = &cli.docs_version {
        settings.docs_version = Some(version.clone());
        settings.docs_dir = None;
    }
    settings.open_browser = cli.open;
    settings
}

fn settings_path() -> PathBuf {
    config::config_dir().join("config.yaml")
}

fn run_panel(panel: Panel) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([780.0, 580.0])
            .with_min_inner_size([640.0, 500.0])
            .with_title("goose-doc"),
        ..Default::default()
    };

    let app = goose_doc::panel::app::PanelApp::new(panel);

    eframe::run_native("goose-doc", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|error| anyhow::anyhow!("failed to open the panel: {error}"))
}

fn print_started(running: &server::RunningServer) {
    println!("Serving goose docs at {}", running.url);
    println!("  listening: {}", running.addr);
    match &running.docs_path {
        Some(path) => println!("  docs root: {}", path.display()),
        None => println!("  docs root: embedded in the binary"),
    }
    if let Some(version) = &running.docs_version {
        println!("  version:   {version} ({} pages)", running.docs_entries);
    }
    if running.serves_network() {
        println!("  reach:     all interfaces (use --local-only to restrict)");
    }
    println!("  goose:     GOOSE_DOCS_ROOT={}", running.url);
}

fn resolve_docs(
    docs_dir: Option<&Path>,
    docs_version: Option<&str>,
    cache_root: &Path,
) -> Result<DocsRoot> {
    docs::resolve(docs_dir, docs_version, cache_root, true)
}

fn report(root: &DocsRoot) {
    if root.is_embedded() {
        println!("docs root: embedded in the binary");
    } else {
        println!("docs root: {}", root.path.display());
    }
    match &root.version {
        Some(version) => println!("version:   {version}"),
        None => println!("version:   unknown (no manifest found)"),
    }
    println!("pages:     {}", root.entries);
    println!("map:       {}", root.map_path());
    println!();
    println!("GOOSE_DOCS_ROOT={}", root.path.display());
}

/// Service management. The docs root is resolved first so the unit references a
/// path that exists, rather than one that only appears after a download.
#[allow(clippy::too_many_arguments)]
fn run_service(
    action: ServiceAction,
    apply: bool,
    start: bool,
    docs_dir: Option<&Path>,
    docs_version: Option<&str>,
    cache: &Path,
    cli: &Cli,
) -> Result<()> {
    let bin = std::env::current_exe().context("failed to locate the goose-doc binary")?;

    // Uninstalling and querying must not depend on a docs root being present:
    // a service is typically removed precisely when the docs are gone or broken.
    let root = match action {
        ServiceAction::Install => Some(resolve_docs(docs_dir, docs_version, cache)?),
        _ => resolve_docs(docs_dir, docs_version, cache).ok(),
    };

    let (docs_path, entries) = match &root {
        Some(root) => (root.path.clone(), root.entries),
        None => (cache.join("bundles").join("unknown"), 0),
    };

    let settings = Settings {
        bind: cli.bind_addr(),
        port: cli.port,
        docs_dir: Some(docs_path.clone()),
        docs_version: None,
        open_browser: false,
    };

    let plan = service::plan(&settings, &docs_path, &bin, cli.port)?;

    match action {
        ServiceAction::Install => {
            println!("service: {}", plan.kind.label());
            println!("location: {}", plan.location.display());
            for line in service::install(&plan, apply, start)? {
                println!("{line}");
            }
            if apply {
                println!();
                println!(
                    "Serving {entries} pages at http://{}:{}",
                    settings.bind, settings.port
                );
            }
        }
        ServiceAction::Uninstall => {
            for line in service::uninstall(&plan, apply)? {
                println!("{line}");
            }
        }
        ServiceAction::Status => {
            println!("service: {}", plan.kind.label());
            println!("location: {}", plan.location.display());
            println!();
            match service::status(&plan) {
                Ok(text) => println!("{text}"),
                Err(error) => println!("could not query status: {error}"),
            }
        }
    }

    Ok(())
}

fn report_addresses() {
    println!("default bind: {}", addr::DEFAULT_BIND);
    println!();
    println!("candidates:");
    for candidate in addr::bind_candidates() {
        match candidate.parse() {
            Ok(ip) => println!("  {:<16} {}", candidate, addr::describe(&ip)),
            Err(_) => println!("  {candidate}"),
        }
    }
    println!();
    match addr::preferred_display_ip() {
        Some(ip) => println!(
            "clients on the network should use: http://{}:{}",
            ip,
            config::DEFAULT_PORT
        ),
        None => println!("no LAN address found; only this machine can connect"),
    }
}
