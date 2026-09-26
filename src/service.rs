//! Service registration, so the docs server survives a reboot on a machine
//! that is meant to host the docs.
//!
//! Each platform gets its native mechanism:
//! - Linux: a systemd unit (system-wide when run as root, user unit otherwise)
//! - macOS: a launchd agent
//! - Windows: a `sc.exe` service
//!
//! Only headless serving is registered: a service has no display, so the panel
//! could never open. The command line is captured verbatim so an operator can
//! see exactly what will run.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::settings::Settings;

/// Unit/agent name. Kept stable so install/uninstall/status agree.
pub const SERVICE_NAME: &str = "goose-doc";

#[derive(Debug, Clone, PartialEq)]
pub struct ServicePlan {
    pub kind: ServiceKind,
    /// Path of the file (or registry key) that backs the service.
    pub location: PathBuf,
    pub contents: String,
    /// Command an operator can run by hand to reproduce the service.
    pub command_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    SystemdUser,
    SystemdSystem,
    Launchd,
    Windows,
}

impl ServiceKind {
    pub fn label(&self) -> &'static str {
        match self {
            ServiceKind::SystemdUser => "systemd (user unit)",
            ServiceKind::SystemdSystem => "systemd (system unit)",
            ServiceKind::Launchd => "launchd (user agent)",
            ServiceKind::Windows => "Windows service",
        }
    }
}

/// Build the service definition for this platform.
///
/// The docs root is passed explicitly, so a service started at boot does not
/// depend on a bundle cache that may not be warm yet.
pub fn plan(settings: &Settings, docs_dir: &Path, bin: &Path, port: u16) -> Result<ServicePlan> {
    let bin = bin
        .to_str()
        .context("binary path is not valid UTF-8")?
        .to_string();
    let docs_dir_text = docs_dir
        .to_str()
        .context("docs root path is not valid UTF-8")?
        .to_string();

    // A service has no display and no terminal, so it must serve headless and
    // must not try to open a browser.
    let args: Vec<String> = vec![
        "--headless".to_string(),
        "--docs-dir".to_string(),
        docs_dir_text.clone(),
        "--port".to_string(),
        port.to_string(),
        "--bind".to_string(),
        settings.bind.clone(),
    ];

    let command_line = std::iter::once(bin.clone())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");

    let hostname = hostname();

    #[cfg(target_os = "linux")]
    {
        let system = is_root();
        let kind = if system {
            ServiceKind::SystemdSystem
        } else {
            ServiceKind::SystemdUser
        };
        let location = if system {
            PathBuf::from(format!("/etc/systemd/system/{SERVICE_NAME}.service"))
        } else {
            config_home()
                .join("systemd")
                .join("user")
                .join(format!("{SERVICE_NAME}.service"))
        };

        let unit = format!(
            "[Unit]\n\
             Description=Serve goose documentation for the goose-doc-guide skill\n\
             Documentation=https://goose-docs.ai/docs/guides/offline-docs\n\
             Wants=network-online.target\n\
             After=network-online.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={command_line}\n\
             Restart=on-failure\n\
             RestartSec=3\n\
             WorkingDirectory={docs_dir}\n\
             \n\
             [Install]\n\
             WantedBy={wanted_by}\n",
            docs_dir = docs_dir_text,
            wanted_by = if system {
                "multi-user.target"
            } else {
                "default.target"
            },
        );

        Ok(ServicePlan {
            kind,
            location,
            contents: unit,
            command_line,
        })
    }

    #[cfg(target_os = "macos")]
    {
        let location = config_home()
            .join("LaunchAgents")
            .join(format!("com.goose-doc.{hostname}.plist"));
        let log_dir = crate::config::cache_dir();
        let stdout = log_dir.join("service.log");
        let stderr = log_dir.join("service.err.log");

        let mut program_args = String::new();
        for arg in std::iter::once(&bin).chain(args.iter()) {
            program_args.push_str(&format!("    <string>{}</string>\n", escape_xml(arg)));
        }

        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             <key>Label</key>\n\
             <string>com.goose-doc.{hostname}</string>\n\
             <key>ProgramArguments</key>\n\
             <array>\n\
             {program_args}\
             </array>\n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
             <key>KeepAlive</key>\n\
             <true/>\n\
             <key>StandardOutPath</key>\n\
             <string>{stdout}</string>\n\
             <key>StandardErrorPath</key>\n\
             <string>{stderr}</string>\n\
             </dict>\n\
             </plist>\n",
            stdout = escape_xml(&stdout.to_string_lossy()),
            stderr = escape_xml(&stderr.to_string_lossy()),
        );

        Ok(ServicePlan {
            kind: ServiceKind::Launchd,
            location,
            contents: plist,
            command_line,
        })
    }

    #[cfg(target_os = "windows")]
    {
        // `sc.exe` needs the executable and arguments quoted separately, and the
        // argument list must keep its quotes or a path with spaces breaks.
        let quoted_args = args
            .iter()
            .map(|arg| quote_windows(arg))
            .collect::<Vec<_>>()
            .join(" ");

        Ok(ServicePlan {
            kind: ServiceKind::Windows,
            location: PathBuf::from(format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{SERVICE_NAME}")),
            contents: format!(
                "sc.exe create {SERVICE_NAME} binPath= \"\\\"{bin}\\\" {quoted_args}\" start= auto\n\
                 sc.exe failure {SERVICE_NAME} reset= 86400 actions= restart/3000/restart/3000/restart/3000\n"
            ),
            command_line,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (hostname, args);
        anyhow::bail!("service installation is not supported on this platform")
    }
}

/// Print the plan, or write it when `apply` is set.
pub fn install(plan: &ServicePlan, apply: bool, enable_now: bool) -> Result<Vec<String>> {
    if !apply {
        return Ok(vec![
            format!("# {} -> {}", plan.kind.label(), plan.location.display()),
            plan.contents.clone(),
            format!(
                "# to apply: goose-doc service install --apply{}",
                if enable_now { " --start" } else { "" }
            ),
        ]);
    }

    if let Some(parent) = plan.location.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    match plan.kind {
        ServiceKind::Windows => {
            // The plan holds two sc.exe invocations; run them as written.
            let mut output = Vec::new();
            for line in plan.contents.lines().filter(|l| !l.trim().is_empty()) {
                let out = run_shell(line).with_context(|| format!("failed to run: {line}"))?;
                output.push(out);
            }
            if enable_now {
                output.push(run_shell(&format!("sc.exe start {SERVICE_NAME}"))?);
            }
            Ok(output)
        }
        _ => {
            std::fs::write(&plan.location, &plan.contents)
                .with_context(|| format!("failed to write {}", plan.location.display()))?;

            let mut output = Vec::new();

            match plan.kind {
                ServiceKind::SystemdUser => {
                    output.push(run("systemctl", &["--user", "daemon-reload"])?);
                    if enable_now {
                        output.push(run(
                            "systemctl",
                            &["--user", "enable", "--now", SERVICE_NAME],
                        )?);
                    } else {
                        output.push(run("systemctl", &["--user", "enable", SERVICE_NAME])?);
                    }
                }
                ServiceKind::SystemdSystem => {
                    output.push(run("systemctl", &["daemon-reload"])?);
                    if enable_now {
                        output.push(run("systemctl", &["enable", "--now", SERVICE_NAME])?);
                    } else {
                        output.push(run("systemctl", &["enable", SERVICE_NAME])?);
                    }
                }
                ServiceKind::Launchd => {
                    // Reload so a changed plist takes effect.
                    let _ = run("launchctl", &["unload", &plan.location.to_string_lossy()]);
                    output.push(run(
                        "launchctl",
                        &["load", &plan.location.to_string_lossy()],
                    )?);
                }
                ServiceKind::Windows => unreachable!("handled above"),
            }

            Ok(output)
        }
    }
}

pub fn uninstall(plan: &ServicePlan, apply: bool) -> Result<Vec<String>> {
    if !apply {
        let hint = match plan.kind {
            ServiceKind::SystemdUser | ServiceKind::SystemdSystem => {
                "goose-doc service uninstall --apply"
            }
            ServiceKind::Launchd => "goose-doc service uninstall --apply",
            ServiceKind::Windows => "goose-doc service uninstall --apply",
        };
        return Ok(vec![
            format!("# would remove {}", plan.location.display()),
            format!("# to apply: {hint}"),
        ]);
    }

    let mut output = Vec::new();

    match plan.kind {
        ServiceKind::SystemdUser => {
            let _ = run("systemctl", &["--user", "disable", "--now", SERVICE_NAME]);
            std::fs::remove_file(&plan.location).ok();
            output.push(run("systemctl", &["--user", "daemon-reload"])?);
        }
        ServiceKind::SystemdSystem => {
            let _ = run("systemctl", &["disable", "--now", SERVICE_NAME]);
            std::fs::remove_file(&plan.location).ok();
            output.push(run("systemctl", &["daemon-reload"])?);
        }
        ServiceKind::Launchd => {
            let _ = run("launchctl", &["unload", &plan.location.to_string_lossy()]);
            std::fs::remove_file(&plan.location).ok();
            output.push("unloaded launchd agent".to_string());
        }
        ServiceKind::Windows => {
            output.push(run_shell(&format!("sc.exe stop {SERVICE_NAME}")).unwrap_or_default());
            output.push(run_shell(&format!("sc.exe delete {SERVICE_NAME}"))?);
        }
    }

    Ok(output)
}

pub fn status(plan: &ServicePlan) -> Result<String> {
    match plan.kind {
        ServiceKind::SystemdUser => run(
            "systemctl",
            &["--user", "status", SERVICE_NAME, "--no-pager"],
        ),
        ServiceKind::SystemdSystem => run("systemctl", &["status", SERVICE_NAME, "--no-pager"]),
        ServiceKind::Launchd => run(
            "launchctl",
            &["list", &format!("com.goose-doc.{}", hostname())],
        ),
        ServiceKind::Windows => run_shell(&format!("sc.exe query {SERVICE_NAME}")),
    }
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(text.trim().to_string())
}

fn run_shell(command_line: &str) -> Result<String> {
    #[cfg(target_os = "windows")]
    {
        run("cmd.exe", &["/C", command_line])
    }
    #[cfg(not(target_os = "windows"))]
    {
        run("sh", &["-c", command_line])
    }
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(std::env::temp_dir)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc_geteuid() == 0 }
}

#[cfg(target_os = "linux")]
extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

#[cfg(target_os = "macos")]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(not(target_os = "macos"))]
fn escape_xml(value: &str) -> String {
    value.to_string()
}

#[cfg(target_os = "windows")]
fn quote_windows(value: &str) -> String {
    if value.contains(' ') {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bin() -> PathBuf {
        PathBuf::from("/opt/goose-doc/goose-doc")
    }

    fn test_settings() -> Settings {
        Settings {
            bind: "0.0.0.0".to_string(),
            port: 10650,
            docs_dir: Some(PathBuf::from("/opt/goose-docs")),
            docs_version: None,
            open_browser: true,
        }
    }

    #[test]
    fn service_always_serves_headless_and_never_opens_a_browser() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");

        assert!(plan.command_line.contains("--headless"));
        assert!(plan.command_line.contains("--docs-dir /opt/goose-docs"));
        assert!(plan.command_line.contains("--port 10650"));
        // The settings asked for a browser, but a service must not try to open one.
        assert!(!plan.command_line.contains("--open"));
    }

    #[test]
    fn service_binds_the_configured_address() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");
        assert!(plan.command_line.contains("--bind 0.0.0.0"));
    }

    /// A plan whose location is inside a fresh temporary directory, so the test
    /// never depends on (or touches) a real service that happens to be
    /// installed on the machine running it.
    fn isolated_plan() -> (ServicePlan, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "goose-doc-service-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");
        plan.location = dir.join("goose-doc.service");
        (plan, dir)
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (plan, dir) = isolated_plan();

        let preview = install(&plan, false, false).expect("preview").join("\n");
        assert!(preview.contains("--headless"));
        assert!(!plan.location.exists(), "dry run wrote a file");

        let preview = uninstall(&plan, false).expect("preview").join("\n");
        assert!(preview.contains("would remove"));

        // Nothing at all should have been created.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_writes_the_unit_and_uninstall_removes_it() {
        let (plan, dir) = isolated_plan();

        // `install --apply` also runs the service manager, which needs privileges
        // and a real unit path; only the file write is exercised here.
        std::fs::write(&plan.location, &plan.contents).expect("write");
        assert!(plan.location.exists());

        std::fs::remove_file(&plan.location).expect("remove");
        assert!(!plan.location.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_dry_run_explains_how_to_apply() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");
        let preview = install(&plan, false, false).expect("preview").join("\n");
        assert!(preview.contains("to apply"), "got: {preview}");
    }

    #[test]
    fn uninstall_dry_run_removes_nothing() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");
        let preview = uninstall(&plan, false).expect("preview").join("\n");
        assert!(preview.contains("would remove"), "got: {preview}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_unit_is_a_valid_unit_with_restart() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");

        assert!(plan.contents.contains("[Unit]"));
        assert!(plan.contents.contains("[Service]"));
        assert!(plan.contents.contains("[Install]"));
        assert!(plan.contents.contains("Restart=on-failure"));
        assert!(plan
            .contents
            .contains("ExecStart=/opt/goose-doc/goose-doc --headless"));
        // The unit path follows the scope: system units live in /etc.
        match plan.kind {
            ServiceKind::SystemdSystem => {
                assert_eq!(
                    plan.location,
                    PathBuf::from("/etc/systemd/system/goose-doc.service")
                )
            }
            ServiceKind::SystemdUser => assert!(plan.location.ends_with("goose-doc.service")),
            other => panic!("unexpected kind {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launchd_plist_has_the_expected_keys() {
        let plan = plan(
            &test_settings(),
            Path::new("/opt/goose-docs"),
            &test_bin(),
            10650,
        )
        .expect("plan");

        for key in [
            "<key>Label</key>",
            "<key>ProgramArguments</key>",
            "<key>RunAtLoad</key>",
            "<key>KeepAlive</key>",
        ] {
            assert!(plan.contents.contains(key), "missing {key}");
        }
        assert!(plan.contents.contains("--headless"));
        assert!(plan.location.extension().is_some_and(|e| e == "plist"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn xml_is_escaped() {
        assert_eq!(escape_xml("a&b<c>"), "a&amp;b&lt;c&gt;");
    }
}
