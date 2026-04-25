use std::{net::Ipv4Addr, path::Path, process::Command};

use serde::Serialize;

use crate::{
    error::{AppError, AppResult},
    model::{DistroFamily, DistroRecord},
};

const REQUIRED_ONLINE_INSTALL_FLAGS: &[&str] =
    &["--name", "--location", "--no-launch", "--web-download"];

#[derive(Debug, Default)]
pub struct WslInventory {
    pub available: bool,
    pub version_banner: Option<String>,
    pub default_distro: Option<String>,
    pub distros: Vec<DistroRecord>,
}

#[derive(Clone, Debug, Default)]
pub struct CommandTranscript {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub exit_code: Option<i32>,
}

impl CommandTranscript {
    pub fn combined_output(&self) -> String {
        match (self.stdout.trim(), self.stderr.trim()) {
            ("", "") => String::new(),
            (stdout, "") => stdout.to_string(),
            ("", stderr) => stderr.to_string(),
            (stdout, stderr) => format!("{stdout}\n\n{stderr}"),
        }
    }

    pub fn require_success(self, program: &str) -> AppResult<String> {
        if self.success {
            Ok(self.stdout)
        } else {
            let exit = self
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Err(AppError::message(format!(
                "{program} exited with {exit}: {}",
                self.combined_output().trim()
            )))
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PasswordStatus {
    Usable,
    Locked,
    Missing,
    Unknown,
}

impl PasswordStatus {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Usable => "usable",
            Self::Locked => "locked",
            Self::Missing => "missing",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_usable(self) -> bool {
        matches!(self, Self::Usable)
    }
}

#[derive(Debug, Default)]
struct OsRelease {
    id: Option<String>,
    id_like: Vec<String>,
    pretty_name: Option<String>,
}

impl OsRelease {
    fn family(&self) -> DistroFamily {
        let mut signals = Vec::new();
        if let Some(id) = &self.id {
            signals.push(id.as_str());
        }
        signals.extend(self.id_like.iter().map(String::as_str));

        if signals.iter().any(|value| value.contains("ubuntu")) {
            DistroFamily::Ubuntu
        } else if signals.iter().any(|value| value.contains("debian")) {
            DistroFamily::Debian
        } else if signals.iter().any(|value| value.contains("fedora")) {
            DistroFamily::Fedora
        } else if signals
            .iter()
            .any(|value| value.contains("arch") || value.contains("manjaro"))
        {
            DistroFamily::Arch
        } else {
            DistroFamily::Unknown
        }
    }
}

pub fn probe_inventory() -> WslInventory {
    let available = Command::new("wsl.exe").arg("--help").output().is_ok();

    if !available {
        return WslInventory::default();
    }

    let version_banner = Command::new("wsl.exe")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| decode_output(&output.stdout))
        .and_then(|raw| {
            raw.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        });

    let verbose_output = Command::new("wsl.exe").args(["-l", "-v"]).output().ok();
    let quiet_output = Command::new("wsl.exe").args(["-l", "-q"]).output().ok();

    let (mut distros, default_distro) = verbose_output
        .as_ref()
        .filter(|output| output.status.success())
        .map(|output| parse_verbose_list(&decode_output(&output.stdout).unwrap_or_default()))
        .unwrap_or_default();

    if distros.is_empty() {
        distros = quiet_output
            .as_ref()
            .filter(|output| output.status.success())
            .map(|output| parse_quiet_list(&decode_output(&output.stdout).unwrap_or_default()))
            .unwrap_or_default();
    }

    WslInventory {
        available,
        version_banner,
        default_distro,
        distros,
    }
}

pub fn inspect_distro(name: &str, inventory: &WslInventory) -> AppResult<DistroRecord> {
    let mut record = inventory
        .distros
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name))
        .cloned()
        .unwrap_or_else(|| DistroRecord {
            name: name.to_string(),
            ..DistroRecord::default()
        });

    if let Ok(os_release_raw) = run_wsl(name, &["cat", "/etc/os-release"]) {
        let os_release = parse_os_release(&os_release_raw);
        record.family = os_release.family();
        record.pretty_name = os_release.pretty_name;
    }

    if let Ok(default_user) = run_wsl(name, &["whoami"]) {
        let user = default_user.trim();
        if !user.is_empty() {
            record.default_user = Some(user.to_string());
        }
    }

    Ok(record)
}

pub fn run_wsl(distro: &str, args: &[&str]) -> AppResult<String> {
    run_wsl_capture(distro, args)?.require_success("wsl.exe")
}

pub fn run_wsl_as_user(distro: &str, user: &str, args: &[&str]) -> AppResult<String> {
    run_wsl_capture_with_user(distro, Some(user), args)?.require_success("wsl.exe")
}

pub fn run_wsl_capture(distro: &str, args: &[&str]) -> AppResult<CommandTranscript> {
    run_wsl_capture_with_user(distro, None, args)
}

pub fn run_wsl_capture_with_user(
    distro: &str,
    user: Option<&str>,
    args: &[&str],
) -> AppResult<CommandTranscript> {
    let mut command = Command::new("wsl.exe");
    command.arg("-d").arg(distro);
    if let Some(user) = user {
        command.arg("-u").arg(user);
    }
    command.arg("--");

    for arg in args {
        command.arg(arg);
    }

    let output = command.output()?;
    Ok(transcript_from_output(output))
}

pub fn import_distro(
    distro: &str,
    install_dir: &Path,
    rootfs_tar: &Path,
) -> AppResult<CommandTranscript> {
    let output = Command::new("wsl.exe")
        .arg("--import")
        .arg(distro)
        .arg(install_dir)
        .arg(rootfs_tar)
        .args(["--version", "2"])
        .output()?;

    Ok(transcript_from_output(output))
}

pub fn install_online_distro(
    distro: &str,
    name: &str,
    install_dir: &Path,
) -> AppResult<CommandTranscript> {
    if let Some(help) = wsl_help_text() {
        let missing_flags = missing_online_install_flags(&help);
        if !missing_flags.is_empty() {
            return Ok(CommandTranscript {
                stdout: String::new(),
                stderr: format!(
                    "This version of wsl.exe does not advertise the flags Pane needs for managed online provisioning: {}. Update WSL from the Microsoft Store or use `pane init --rootfs-tar <path>`.",
                    missing_flags.join(", ")
                ),
                success: false,
                exit_code: Some(1),
            });
        }
    }

    let output = Command::new("wsl.exe")
        .arg("--install")
        .arg(distro)
        .arg("--name")
        .arg(name)
        .arg("--location")
        .arg(install_dir)
        .arg("--no-launch")
        .arg("--web-download")
        .output()?;

    Ok(transcript_from_output(output))
}

fn wsl_help_text() -> Option<String> {
    Command::new("wsl.exe")
        .arg("--help")
        .output()
        .ok()
        .and_then(|output| {
            let transcript = transcript_from_output(output);
            let combined = transcript.combined_output();
            (!combined.trim().is_empty()).then_some(combined)
        })
}

fn missing_online_install_flags(help: &str) -> Vec<&'static str> {
    REQUIRED_ONLINE_INSTALL_FLAGS
        .iter()
        .copied()
        .filter(|flag| !help.contains(flag))
        .collect()
}

pub fn unregister_distro(distro: &str) -> AppResult<CommandTranscript> {
    let output = Command::new("wsl.exe")
        .arg("--unregister")
        .arg(distro)
        .output()?;

    Ok(transcript_from_output(output))
}

pub fn open_interactive_terminal(distro: &str, user: Option<&str>) -> AppResult<()> {
    let mut command = Command::new("wsl.exe");
    command.arg("-d").arg(distro);
    if let Some(user) = user {
        command.arg("-u").arg(user);
    }

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        let exit = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Err(AppError::message(format!(
            "wsl.exe interactive terminal exited with {exit} for '{distro}'."
        )))
    }
}

pub fn run_wsl_shell_as_user(
    distro: &str,
    user: Option<&str>,
    shell_command: &str,
) -> AppResult<String> {
    run_wsl_shell_as_user_capture(distro, user, shell_command)?.require_success("wsl.exe")
}

pub fn run_wsl_shell_as_user_capture(
    distro: &str,
    user: Option<&str>,
    shell_command: &str,
) -> AppResult<CommandTranscript> {
    let normalized_command = normalize_shell_command(shell_command);
    let mut command = Command::new("wsl.exe");
    command.arg("-d").arg(distro);
    if let Some(user) = user {
        command.arg("-u").arg(user);
    }
    command
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg(&normalized_command);

    let output = command.output()?;
    Ok(transcript_from_output(output))
}

pub fn run_wsl_shell_as_user_capture_with_input(
    distro: &str,
    user: Option<&str>,
    shell_command: &str,
    stdin_input: &str,
) -> AppResult<CommandTranscript> {
    use std::io::Write;
    use std::process::Stdio;

    let normalized_command = normalize_shell_command(shell_command);
    let mut command = Command::new("wsl.exe");
    command.arg("-d").arg(distro);
    if let Some(user) = user {
        command.arg("-u").arg(user);
    }
    command
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg(&normalized_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok(transcript_from_output(output))
}

pub fn shutdown_wsl() -> AppResult<CommandTranscript> {
    let output = Command::new("wsl.exe").arg("--shutdown").output()?;
    Ok(transcript_from_output(output))
}

pub fn distro_command_exists(distro: &str, binary: &str) -> bool {
    let command = format!(
        "command -v {} >/dev/null 2>&1 && echo yes || echo no",
        shell_quote(binary)
    );
    matches!(
        run_wsl_shell_as_user(distro, None, &command)
            .ok()
            .as_deref()
            .map(str::trim),
        Some("yes")
    )
}

pub fn distro_file_exists(distro: &str, path: &str) -> bool {
    let command = format!("[ -f \"$HOME/{path}\" ] && echo yes || echo no");
    matches!(
        run_wsl_shell_as_user(distro, None, &command)
            .ok()
            .as_deref()
            .map(str::trim),
        Some("yes")
    )
}

pub fn distro_service_active(distro: &str, service: &str) -> Option<bool> {
    let command = format!(
        "if command -v systemctl >/dev/null 2>&1; then systemctl is-active {service} >/dev/null 2>&1 && echo yes || echo no; elif command -v service >/dev/null 2>&1; then service {service} status >/dev/null 2>&1 && echo yes || echo no; elif command -v pgrep >/dev/null 2>&1; then pgrep -x {service} >/dev/null 2>&1 && echo yes || echo no; else echo unknown; fi",
        service = shell_quote(service)
    );
    yes_no_unknown(
        run_wsl_shell_as_user(distro, None, &command)
            .ok()
            .as_deref(),
    )
}

pub fn distro_port_listening(distro: &str, port: u16) -> Option<bool> {
    let command = format!(
        "if command -v ss >/dev/null 2>&1; then ss -ltnH 'sport = :{port}' >/dev/null 2>&1 && echo yes || echo no; elif command -v netstat >/dev/null 2>&1; then netstat -ltn | awk '{{print $4}}' | grep -Eq '(^|:){port}$' && echo yes || echo no; else echo unknown; fi"
    );
    yes_no_unknown(
        run_wsl_shell_as_user(distro, None, &command)
            .ok()
            .as_deref(),
    )
}

pub fn distro_ipv4_address(distro: &str) -> Option<Ipv4Addr> {
    let command = "if command -v ip >/dev/null 2>&1; then ip -o -4 addr show scope global | grep -v ' lo' | head -n 1 | grep -Eo \"([0-9]{1,3}\\.){3}[0-9]{1,3}\" | head -n 1; else echo ''; fi";
    run_wsl_shell_as_user(distro, None, command)
        .ok()
        .and_then(|output| parse_ipv4_address(&output))
}

pub fn distro_systemd_configured(distro: &str) -> Option<bool> {
    let command = "if [ -f /etc/wsl.conf ] && grep -Eq '^[[:space:]]*systemd[[:space:]]*=[[:space:]]*true[[:space:]]*$' /etc/wsl.conf; then echo yes; else echo no; fi";
    let output = run_wsl_shell_as_user(distro, Some("root"), command).ok();
    yes_no_unknown(output.as_deref())
}

pub fn distro_systemd_active(distro: &str) -> Option<bool> {
    let command = "if [ -d /run/systemd/system ] && [ \"$(ps -p 1 -o comm= 2>/dev/null | tr -d '[:space:]')\" = \"systemd\" ]; then echo yes; else echo no; fi";
    yes_no_unknown(run_wsl_shell_as_user(distro, None, command).ok().as_deref())
}

pub fn distro_user_password_status(distro: &str, user: &str) -> Option<PasswordStatus> {
    let command = format!("passwd -S {} 2>/dev/null || true", shell_quote(user));
    let output = run_wsl_shell_as_user(distro, Some("root"), &command).ok()?;
    parse_password_status(&output)
}

pub fn distro_pane_session_assets_ready(distro: &str, user: &str) -> Option<bool> {
    let target_home = distro_user_home_path(distro, user)?;
    let xsession = format!("{target_home}/.xsession");
    let xinitrc = format!("{target_home}/.xinitrc");
    let pane_session_start = format!("{target_home}/.pane-session-start");
    let notifyd_override =
        format!("{target_home}/.config/systemd/user/xfce4-notifyd.service.d/pane-x11.conf");

    for path in [&xsession, &xinitrc, &pane_session_start, &notifyd_override] {
        if !run_wsl_command_as_user_succeeds(distro, user, &["test", "-f", path])? {
            return Some(false);
        }
    }

    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &["grep", "-Fq", ".pane-session-start", &xsession],
    )? {
        return Some(false);
    }
    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &["grep", "-Fq", ".pane-session-start", &xinitrc],
    )? {
        return Some(false);
    }
    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &[
            "grep",
            "-Fq",
            "dbus-run-session -- startxfce4",
            &pane_session_start,
        ],
    )? {
        return Some(false);
    }
    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &["grep", "-Fq", "XDG_SESSION_TYPE=x11", &pane_session_start],
    )? {
        return Some(false);
    }
    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &["grep", "-Fq", "GDK_BACKEND=x11", &pane_session_start],
    )? {
        return Some(false);
    }
    if !run_wsl_command_as_user_succeeds(
        distro,
        user,
        &[
            "grep",
            "-Fq",
            "Environment=GDK_BACKEND=x11",
            &notifyd_override,
        ],
    )? {
        return Some(false);
    }

    Some(true)
}

pub fn distro_user_home_ready(distro: &str, user: &str) -> Option<bool> {
    let target_home = distro_user_home_path(distro, user)?;
    let uid = distro_user_numeric_id(distro, user, "-u")?;
    let gid = distro_user_numeric_id(distro, user, "-g")?;
    let expected_owner = format!("{uid}:{gid}");
    let required_paths = [
        format!("{target_home}/.config"),
        format!("{target_home}/.config/dconf"),
        format!("{target_home}/.config/Thunar"),
        format!("{target_home}/.config/xfce4"),
        format!("{target_home}/.config/xfce4/panel"),
        format!("{target_home}/.cache"),
        format!("{target_home}/.local"),
        format!("{target_home}/.local/share"),
        format!("{target_home}/.local/state"),
    ];

    for path in &required_paths {
        if !run_wsl_command_as_user_succeeds(distro, user, &["test", "-d", path])? {
            return Some(false);
        }
        let owner = run_wsl_as_user(distro, user, &["stat", "-c", "%u:%g", path])
            .ok()?
            .trim()
            .to_string();
        if owner != expected_owner {
            return Some(false);
        }
        if !run_wsl_command_as_user_succeeds(distro, user, &["test", "-w", path])? {
            return Some(false);
        }
    }

    Some(true)
}

fn distro_user_home_path(distro: &str, user: &str) -> Option<String> {
    run_wsl_as_user(distro, user, &["printenv", "HOME"])
        .ok()
        .and_then(|output| {
            output
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
}

fn distro_user_numeric_id(distro: &str, user: &str, flag: &str) -> Option<String> {
    run_wsl_as_user(distro, user, &["id", flag])
        .ok()
        .and_then(|output| {
            output
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
}

fn run_wsl_command_as_user_succeeds(distro: &str, user: &str, args: &[&str]) -> Option<bool> {
    Some(
        run_wsl_capture_with_user(distro, Some(user), args)
            .ok()?
            .success,
    )
}
pub fn stop_xrdp_services(distro: &str) -> AppResult<String> {
    let command = "if command -v systemctl >/dev/null 2>&1; then systemctl stop xrdp.service xrdp-sesman.service >/dev/null 2>&1 || true; fi
if command -v service >/dev/null 2>&1; then service xrdp stop >/dev/null 2>&1 || true; fi
pkill -x xrdp >/dev/null 2>&1 || true
pkill -x xrdp-sesman >/dev/null 2>&1 || true
echo stopped";

    run_wsl_shell_as_user(distro, Some("root"), command)
}

pub fn remove_pane_session_assets(distro: &str, user: &str) -> AppResult<String> {
    let command = format!(
        "target_home=\"$(getent passwd {user} | cut -d: -f6)\"\nif [ -z \"$target_home\" ]; then\n  echo unknown-user\n  exit 0\nfi\nremoved=0\npreserved=0\nremove_if_pane_managed() {{\n  path=\"$1\"\n  if [ ! -f \"$path\" ]; then\n    return 0\n  fi\n  content=\"$(tr -d '\\r' < \"$path\" | tr -d '\\n')\"\n  case \"$content\" in\n    *'.pane-session-start'*|startxfce4|startplasma-x11|gnome-session)\n      rm -f \"$path\"\n      removed=1\n      ;;\n    *)\n      preserved=1\n      ;;\n  esac\n}}\nremove_if_pane_managed \"$target_home/.xsession\"\nremove_if_pane_managed \"$target_home/.xinitrc\"\nif [ -f \"$target_home/.pane-session-start\" ]; then\n  rm -f \"$target_home/.pane-session-start\"\n  removed=1\nfi\noverride_dir=\"$target_home/.config/systemd/user/xfce4-notifyd.service.d\"\noverride_path=\"$override_dir/pane-x11.conf\"\nif [ -f \"$override_path\" ]; then\n  rm -f \"$override_path\"\n  removed=1\nfi\nif [ -d \"$override_dir\" ] && [ -z \"$(ls -A \"$override_dir\" 2>/dev/null)\" ]; then\n  rmdir \"$override_dir\" 2>/dev/null || true\nfi\nrunuser -u {user} -- systemctl --user daemon-reload >/dev/null 2>&1 || true\nif [ \"$removed\" = 1 ]; then\n  echo removed\nelif [ \"$preserved\" = 1 ]; then\n  echo preserved\nelse\n  echo absent\nfi",
        user = shell_quote(user)
    );

    run_wsl_shell_as_user(distro, Some("root"), &command)
}
pub fn tail_xrdp_logs(distro: &str, lines: usize) -> AppResult<String> {
    let lines = lines.max(1);
    let command = format!(
        "if [ -d /run/systemd/system ] && command -v journalctl >/dev/null 2>&1; then
  journalctl -u xrdp -u xrdp-sesman -n {lines} --no-pager 2>/dev/null || true
else
  for file in /var/log/xrdp.log /var/log/xrdp-sesman.log; do
    if [ -f \"$file\" ]; then
      echo \"==> $file <==\"
      tail -n {lines} \"$file\"
      echo
    fi
  done
fi"
    );

    run_wsl_shell_as_user(distro, Some("root"), &command)
}

fn normalize_shell_command(command: &str) -> String {
    command.replace("\r\n", "\n").replace('\r', "\n")
}
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn parse_ipv4_address(raw: &str) -> Option<Ipv4Addr> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| line.parse::<Ipv4Addr>().ok())
}

fn yes_no_unknown(output: Option<&str>) -> Option<bool> {
    match output.map(|value| value.trim()) {
        Some("yes") => Some(true),
        Some("no") => Some(false),
        _ => None,
    }
}

fn parse_verbose_list(raw: &str) -> (Vec<DistroRecord>, Option<String>) {
    let mut default_distro = None;
    let mut distros = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("NAME") {
            continue;
        }

        let default = trimmed.starts_with('*');
        let normalized = trimmed.trim_start_matches('*').trim();
        let columns: Vec<&str> = normalized.split_whitespace().collect();
        if columns.len() < 3 {
            continue;
        }

        let version = columns.last().map(ToString::to_string);
        let state = columns
            .get(columns.len().saturating_sub(2))
            .map(ToString::to_string);
        let name = columns[..columns.len() - 2].join(" ");

        if default {
            default_distro = Some(name.clone());
        }

        distros.push(DistroRecord {
            name,
            state,
            version,
            default,
            ..DistroRecord::default()
        });
    }

    (distros, default_distro)
}

fn parse_quiet_list(raw: &str) -> Vec<DistroRecord> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|name| DistroRecord {
            name: name.to_string(),
            ..DistroRecord::default()
        })
        .collect()
}

fn parse_os_release(raw: &str) -> OsRelease {
    let mut os_release = OsRelease::default();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());

        match key {
            "ID" => os_release.id = Some(value.to_ascii_lowercase()),
            "ID_LIKE" => {
                os_release.id_like = value
                    .split_whitespace()
                    .map(|item| item.to_ascii_lowercase())
                    .collect();
            }
            "PRETTY_NAME" => os_release.pretty_name = Some(value),
            _ => {}
        }
    }

    os_release
}

fn parse_password_status(raw: &str) -> Option<PasswordStatus> {
    let token = raw.split_whitespace().nth(1)?;
    let status = if token.starts_with('P') {
        PasswordStatus::Usable
    } else if token.starts_with('L') {
        PasswordStatus::Locked
    } else if token.starts_with('N') {
        PasswordStatus::Missing
    } else {
        PasswordStatus::Unknown
    };

    Some(status)
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_string()
}

fn transcript_from_output(output: std::process::Output) -> CommandTranscript {
    CommandTranscript {
        stdout: decode_output(&output.stdout).unwrap_or_default(),
        stderr: decode_output(&output.stderr).unwrap_or_default(),
        success: output.status.success(),
        exit_code: output.status.code(),
    }
}

fn decode_output(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let text = if looks_like_utf16(bytes) {
        let utf16: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&utf16).ok()
    } else {
        Some(String::from_utf8_lossy(bytes).to_string())
    }?;

    Some(text.replace('\u{feff}', ""))
}

fn looks_like_utf16(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes.chunks_exact(2).take(8).any(|chunk| chunk[1] == 0)
}

#[cfg(test)]
mod tests {
    use crate::model::DistroFamily;

    use super::{
        decode_output, missing_online_install_flags, normalize_shell_command, parse_ipv4_address,
        parse_os_release, parse_password_status, parse_verbose_list, yes_no_unknown,
        PasswordStatus,
    };

    #[test]
    fn parses_wsl_verbose_listing() {
        let raw =
            "  NAME            STATE           VERSION\n* Ubuntu-24.04    Running         2\n  Arch            Stopped         2\n";
        let (distros, default_distro) = parse_verbose_list(raw);

        assert_eq!(default_distro.as_deref(), Some("Ubuntu-24.04"));
        assert_eq!(distros.len(), 2);
        assert_eq!(distros[1].name, "Arch");
        assert_eq!(distros[1].state.as_deref(), Some("Stopped"));
    }

    #[test]
    fn parses_os_release_family() {
        let raw = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n";
        let os_release = parse_os_release(raw);

        assert_eq!(os_release.family(), DistroFamily::Ubuntu);
        assert_eq!(os_release.pretty_name.as_deref(), Some("Ubuntu 24.04 LTS"));
    }

    #[test]
    fn decodes_utf16_wsl_output() {
        let bytes: Vec<u8> = "WSL version: 2.6.1.0\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();

        assert_eq!(
            decode_output(&bytes).as_deref(),
            Some("WSL version: 2.6.1.0\r\n")
        );
    }

    #[test]
    fn normalizes_shell_commands_to_lf() {
        assert_eq!(
            normalize_shell_command("echo hi\r\nwhoami\rbye"),
            "echo hi\nwhoami\nbye"
        );
    }
    #[test]
    fn maps_yes_no_unknown_outputs() {
        assert_eq!(yes_no_unknown(Some(&"yes".to_string())), Some(true));
        assert_eq!(yes_no_unknown(Some(&"no".to_string())), Some(false));
        assert_eq!(yes_no_unknown(Some(&"unknown".to_string())), None);
    }

    #[test]
    fn parses_ipv4_address_tokens() {
        assert_eq!(
            parse_ipv4_address("172.24.78.166\n"),
            Some(std::net::Ipv4Addr::new(172, 24, 78, 166))
        );
        assert_eq!(parse_ipv4_address("\n\n"), None);
    }

    #[test]
    fn parses_password_status_tokens() {
        assert_eq!(
            parse_password_status("afsah P 2024-01-01 0 99999 7 -1"),
            Some(PasswordStatus::Usable)
        );
        assert_eq!(
            parse_password_status("afsah L 2024-01-01 0 99999 7 -1"),
            Some(PasswordStatus::Locked)
        );
        assert_eq!(
            parse_password_status("afsah NP 2024-01-01 0 99999 7 -1"),
            Some(PasswordStatus::Missing)
        );
    }

    #[test]
    fn detects_missing_online_install_flags_from_wsl_help() {
        let current_help = "Usage: wsl.exe --install --name --location --no-launch --web-download";
        assert!(missing_online_install_flags(current_help).is_empty());

        let old_help = "Usage: wsl.exe --install --distribution";
        assert_eq!(
            missing_online_install_flags(old_help),
            vec!["--name", "--location", "--no-launch", "--web-download"]
        );
    }
}
