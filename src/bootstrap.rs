#![allow(clippy::uninlined_format_args)]

use crate::model::{DesktopEnvironment, DistroFamily, DistroRecord};

enum BootstrapMode {
    Ensure,
    Update,
}

pub fn render_bootstrap_script(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
    shared_directory_wsl: &str,
) -> String {
    render_arch_xfce_script(
        distro,
        desktop_environment,
        port,
        shared_directory_wsl,
        BootstrapMode::Ensure,
    )
}

pub fn render_update_script(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
    shared_directory_wsl: &str,
) -> String {
    render_arch_xfce_script(
        distro,
        desktop_environment,
        port,
        shared_directory_wsl,
        BootstrapMode::Update,
    )
}

fn render_arch_xfce_script(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
    shared_directory_wsl: &str,
    mode: BootstrapMode,
) -> String {
    if distro.family != DistroFamily::Arch || desktop_environment != DesktopEnvironment::Xfce {
        return unsupported_script(distro, desktop_environment);
    }

    let official_packages = [
        "sudo",
        "git",
        "base-devel",
        "xorg-xinit",
        "dbus",
        "openbsd-netcat",
        "xfce4",
        "xfce4-goodies",
    ];
    let package_commands = match mode {
        BootstrapMode::Ensure => format!(
            "  pacman -Syu --noconfirm\n  install_official_packages {}\n  ensure_xrdp_packages",
            official_packages.join(" ")
        ),
        BootstrapMode::Update => format!(
            "  pacman -Syu --noconfirm\n  install_official_packages {}\n  ensure_xrdp_packages",
            official_packages.join(" ")
        ),
    };
    let intro_message = match mode {
        BootstrapMode::Ensure => "Installing XFCE prerequisites for Arch Linux.",
        BootstrapMode::Update => {
            "Refreshing Arch packages and reapplying the Pane desktop integration."
        }
    };
    let completion_message = match mode {
        BootstrapMode::Ensure => {
            "Pane bootstrap completed. Shared files are available through ~/PaneShared."
        }
        BootstrapMode::Update => {
            "Pane update completed. Reconnect to pick up the refreshed Arch environment."
        }
    };
    let shared_directory_wsl = shell_single_quote(shared_directory_wsl);

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

pane_shared_dir="${{PANE_SHARED_DIR:-{shared_directory_wsl}}}"

log() {{
  printf '[pane] %s\n' "$1"
}}

require_root() {{
  if [ "$(id -u)" -ne 0 ]; then
    echo "pane bootstrap must run as root" >&2
    exit 1
  fi
}}

require_systemd() {{
  if ! command -v systemctl >/dev/null 2>&1 || [ ! -d /run/systemd/system ]; then
    echo "Pane MVP requires Arch Linux on WSL with systemd enabled. Add [boot]/systemd=true to /etc/wsl.conf, then run wsl.exe --shutdown and retry." >&2
    exit 1
  fi
}}

ensure_pacman_keyring() {{
  install -d -m 0700 /etc/pacman.d/gnupg
  chown root:root /etc/pacman.d/gnupg
  if ! pacman-key --list-keys >/dev/null 2>&1; then
    log "Initializing the pacman keyring for Arch Linux."
    pacman-key --init
    pacman-key --populate archlinux
  fi
}}

install_official_packages() {{
  pacman -S --needed --noconfirm "$@"
}}

ensure_xrdp_packages() {{
  if pacman -Si xrdp >/dev/null 2>&1 && pacman -Si xorgxrdp >/dev/null 2>&1; then
    pacman -S --needed --noconfirm xrdp xorgxrdp
    return
  fi

  echo "Pane requires xrdp and xorgxrdp from trusted package sources. This build does not compile AUR packages automatically." >&2
  echo "Use a Pane-approved Arch image or package source that provides xrdp and xorgxrdp, then rerun pane repair." >&2
  exit 1
}}

resolve_target_home() {{
  local user="$1"
  local home_dir

  home_dir="$(getent passwd "$user" | cut -d: -f6)"
  if [ -z "$home_dir" ]; then
    echo "could not resolve a home directory for $user" >&2
    exit 1
  fi

  printf '%s' "$home_dir"
}}

ensure_target_user_dirs() {{
  local target_user="${{PANE_TARGET_USER:-}}"
  local target_home

  if [ -z "$target_user" ]; then
    echo "PANE_TARGET_USER is required" >&2
    exit 1
  fi

  target_home="$(resolve_target_home "$target_user")"
  install -d -m 0700 -o "$target_user" -g "$target_user" \
    "${{target_home}}/.config" \
    "${{target_home}}/.config/dconf" \
    "${{target_home}}/.config/Thunar" \
    "${{target_home}}/.config/xfce4/panel" \
    "${{target_home}}/.cache" \
    "${{target_home}}/.local" \
    "${{target_home}}/.local/share" \
    "${{target_home}}/.local/state"
  chown -R "$target_user:$target_user" "${{target_home}}/.config" "${{target_home}}/.cache" "${{target_home}}/.local"
}}

write_xsession() {{
  local target_user="${{PANE_TARGET_USER:-}}"
  local target_home
  local session_launcher

  if [ -z "$target_user" ]; then
    echo "PANE_TARGET_USER is required" >&2
    exit 1
  fi

  target_home="$(resolve_target_home "$target_user")"
  session_launcher="${{target_home}}/.pane-session-start"
  cat > "$session_launcher" <<'EOF'
#!/usr/bin/env bash
unset WAYLAND_DISPLAY
unset WAYLAND_SOCKET
export XDG_SESSION_TYPE=x11
export GDK_BACKEND=x11
export XDG_CONFIG_HOME="${{HOME}}/.config"
export XDG_CACHE_HOME="${{HOME}}/.cache"
export XDG_DATA_HOME="${{HOME}}/.local/share"
export XDG_STATE_HOME="${{HOME}}/.local/state"
export XDG_CONFIG_DIRS="/etc/xdg"
export XDG_DATA_DIRS="/usr/local/share:/usr/share"
export XDG_MENU_PREFIX="xfce-"
export DESKTOP_SESSION="xfce"
export XDG_CURRENT_DESKTOP="XFCE"
export XFCE4HOME="${{XDG_CONFIG_HOME}}/xfce4"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CONFIG_HOME/dconf" "$XDG_CONFIG_HOME/Thunar" "$XDG_CONFIG_HOME/xfce4/panel" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
if command -v systemctl >/dev/null 2>&1; then
  systemctl --user set-environment \
    GDK_BACKEND="$GDK_BACKEND" \
    XDG_SESSION_TYPE="$XDG_SESSION_TYPE" \
    XDG_CONFIG_HOME="$XDG_CONFIG_HOME" \
    XDG_CACHE_HOME="$XDG_CACHE_HOME" \
    XDG_DATA_HOME="$XDG_DATA_HOME" \
    XDG_STATE_HOME="$XDG_STATE_HOME" \
    XDG_CONFIG_DIRS="$XDG_CONFIG_DIRS" \
    XDG_DATA_DIRS="$XDG_DATA_DIRS" \
    XDG_MENU_PREFIX="$XDG_MENU_PREFIX" \
    DESKTOP_SESSION="$DESKTOP_SESSION" \
    XDG_CURRENT_DESKTOP="$XDG_CURRENT_DESKTOP" \
    XFCE4HOME="$XFCE4HOME" \
    DISPLAY="${{DISPLAY:-:0}}" >/dev/null 2>&1 || true
fi
exec dbus-run-session -- startxfce4
EOF
  chmod 0755 "$session_launcher"
  printf '%s\n' "exec \"$session_launcher\"" > "${{target_home}}/.xsession"
  printf '%s\n' "exec \"$session_launcher\"" > "${{target_home}}/.xinitrc"
  chown "$target_user:$target_user" "$session_launcher" "${{target_home}}/.xsession" "${{target_home}}/.xinitrc"
}}

configure_notifyd_for_x11() {{
  local target_user="${{PANE_TARGET_USER:-}}"
  local target_home
  local override_dir
  local override_path

  if [ -z "$target_user" ]; then
    echo "PANE_TARGET_USER is required" >&2
    exit 1
  fi

  target_home="$(resolve_target_home "$target_user")"
  override_dir="${{target_home}}/.config/systemd/user/xfce4-notifyd.service.d"
  override_path="${{override_dir}}/pane-x11.conf"

  install -d -m 0755 -o "$target_user" -g "$target_user" "$override_dir"
  cat > "$override_path" <<'EOF'
[Service]
Environment=GDK_BACKEND=x11
Environment=XDG_SESSION_TYPE=x11
Environment=WAYLAND_DISPLAY=
Environment=WAYLAND_SOCKET=
EOF
  chown "$target_user:$target_user" "$override_path"
  runuser -u "$target_user" -- systemctl --user daemon-reload >/dev/null 2>&1 || true
}}
write_low_latency_xfce_profile() {{
  local target_user="${{PANE_TARGET_USER:-}}"
  local target_home
  local config_dir
  local config_path

  target_home="$(resolve_target_home "$target_user")"
  config_dir="${{target_home}}/.config/xfce4/xfconf/xfce-perchannel-xml"
  config_path="${{config_dir}}/xfwm4.xml"

  install -d -m 0755 -o "$target_user" -g "$target_user" "$config_dir"
  cat > "$config_path" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<channel name="xfwm4" version="1.0">
  <property name="general" type="empty">
    <property name="use_compositing" type="bool" value="false"/>
  </property>
</channel>
EOF
  chown "$target_user:$target_user" "$config_path"
}}

configure_shared_directory() {{
  local target_user="${{PANE_TARGET_USER:-}}"
  local target_home
  local target_link
  local desktop_link

  target_home="$(resolve_target_home "$target_user")"
  target_link="${{target_home}}/PaneShared"
  desktop_link="${{target_home}}/Desktop/Pane Shared"

  mkdir -p "$pane_shared_dir"
  ln -sfn "$pane_shared_dir" "$target_link"
  chown -h "$target_user:$target_user" "$target_link" 2>/dev/null || true

  if [ -d "${{target_home}}/Desktop" ]; then
    ln -sfn "$target_link" "$desktop_link"
    chown -h "$target_user:$target_user" "$desktop_link" 2>/dev/null || true
  fi
}}

configure_xrdp_port() {{
  if [ -f /etc/xrdp/xrdp.ini ]; then
    awk -v listen_port="{port}" '
function flush_xorg_defaults() {{
  if (section == "Xorg") {{
    if (!xorg_ip) print "ip=127.0.0.1"
    if (!xorg_port) print "port=-1"
    if (!xorg_code) print "code=20"
  }}
}}
/^[[][^]]+[]]$/ {{
  flush_xorg_defaults()
  section = substr($0, 2, length($0) - 2)
  if (section == "Xorg") {{
    xorg_ip = 0
    xorg_port = 0
    xorg_code = 0
  }}
  print
  next
}}
{{
  if (section == "Globals" && $0 ~ /^port=/ && !global_port_done) {{
    print "port=" listen_port
    global_port_done = 1
    next
  }}
  if (section == "Xorg") {{
    if ($0 ~ /^ip=/) {{
      print "ip=127.0.0.1"
      xorg_ip = 1
      next
    }}
    if ($0 ~ /^port=/) {{
      print "port=-1"
      xorg_port = 1
      next
    }}
    if ($0 ~ /^code=/) {{
      print "code=20"
      xorg_code = 1
      next
    }}
  }}
  print
}}
END {{
  flush_xorg_defaults()
}}
' /etc/xrdp/xrdp.ini > /etc/xrdp/xrdp.ini.pane && mv /etc/xrdp/xrdp.ini.pane /etc/xrdp/xrdp.ini
  fi
}}
ensure_service() {{
  systemctl enable xrdp >/dev/null 2>&1 || true
  systemctl restart xrdp
}}

main() {{
  require_root
  require_systemd
  ensure_pacman_keyring
  log "{intro_message}"
{package_commands}
  mkdir -p /var/run/xrdp
  configure_xrdp_port
  ensure_target_user_dirs
  write_xsession
  configure_notifyd_for_x11
  write_low_latency_xfce_profile
  configure_shared_directory
  ensure_service
  log "{completion_message}"
}}

main "$@"
"#,
        completion_message = completion_message,
        intro_message = intro_message,
        package_commands = package_commands,
        port = port,
        shared_directory_wsl = shared_directory_wsl,
    )
}

fn unsupported_script(distro: &DistroRecord, desktop_environment: DesktopEnvironment) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

echo "Pane MVP currently supports Arch Linux + XFCE only." >&2
echo "Requested distro family: {family}" >&2
echo "Requested desktop: {desktop}" >&2
exit 1
"#,
        family = distro.family.display_name(),
        desktop = desktop_environment.display_name(),
    )
}

fn shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}

#[cfg(test)]
mod tests {
    use super::{render_bootstrap_script, render_update_script};
    use crate::model::{DesktopEnvironment, DistroFamily, DistroRecord};

    #[test]
    fn renders_arch_xfce_script_for_mvp() {
        let script = render_bootstrap_script(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
            "/mnt/c/Users/Afsah/AppData/Local/Pane/sessions/pane/shared",
        );

        assert!(script.contains(
            "install_official_packages sudo git base-devel xorg-xinit dbus openbsd-netcat xfce4 xfce4-goodies"
        ));
        assert!(script.contains("ensure_xrdp_packages"));
        assert!(script.contains("does not compile AUR packages automatically"));
        assert!(!script.contains("--skippgpcheck"));
        assert!(!script.contains("https://aur.archlinux.org/${package}.git"));
        assert!(script.contains("systemctl restart xrdp"));
        assert!(script.contains("exec dbus-run-session -- startxfce4"));
        assert!(script.contains(".pane-session-start"));
        assert!(script.contains("GDK_BACKEND=x11"));
        assert!(script.contains("XDG_SESSION_TYPE=x11"));
        assert!(script.contains("XDG_CONFIG_HOME"));
        assert!(script.contains("systemctl --user set-environment"));
        assert!(script.contains(".config/dconf"));
        assert!(script.contains("xfce4-notifyd.service.d"));
        assert!(script.contains(".xinitrc"));
        assert!(script.contains("listen_port=\"3390\""));
        assert!(script.contains("ip=127.0.0.1"));
        assert!(script.contains("port=-1"));
        assert!(script.contains("PaneShared"));
        assert!(script.contains("use_compositing"));
    }

    #[test]
    fn renders_arch_update_script_for_mvp() {
        let script = render_update_script(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
            "/mnt/c/Users/Afsah/AppData/Local/Pane/sessions/pane/shared",
        );

        assert!(script.contains("pacman -Syu --noconfirm"));
        assert!(script.contains(
            "install_official_packages sudo git base-devel xorg-xinit dbus openbsd-netcat xfce4 xfce4-goodies"
        ));
        assert!(script.contains("ensure_xrdp_packages"));
        assert!(script.contains("Pane update completed"));
    }

    #[test]
    fn renders_non_arch_or_non_xfce_script_as_unsupported() {
        let script = render_bootstrap_script(
            &DistroRecord {
                name: "ubuntu".to_string(),
                family: DistroFamily::Ubuntu,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
            "/mnt/c/shared",
        );

        assert!(script.contains("Arch Linux + XFCE only"));
        assert!(script.contains("exit 1"));
    }
}
