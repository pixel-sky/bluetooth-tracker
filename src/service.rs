use crate::{
    address::BluetoothAddress,
    paths::{default_user_systemd_dir, TrackerPaths},
};
use anyhow::{anyhow, Context, Result};
use std::{env, ffi::OsStr, fs, path::Path, process::Command};

const SERVICE_NAME: &str = "keychron-tracker.service";

pub fn install(address: &BluetoothAddress, paths: &TrackerPaths) -> Result<()> {
    let service_dir = default_user_systemd_dir()?;
    fs::create_dir_all(&service_dir)
        .with_context(|| format!("failed to create {}", service_dir.display()))?;

    let service_path = service_dir.join(SERVICE_NAME);
    let exe = env::current_exe().context("failed to locate current executable")?;
    let unit = render_unit(&exe, address, paths);
    fs::write(&service_path, unit)
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", "--now", SERVICE_NAME])?;

    println!("Installed {}", service_path.display());
    println!("Status: systemctl --user status keychron-tracker");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let service_dir = default_user_systemd_dir()?;
    let service_path = service_dir.join(SERVICE_NAME);

    let _ = run_systemctl(&["--user", "disable", "--now", SERVICE_NAME]);
    match fs::remove_file(&service_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| format!("failed to remove {}", service_path.display()))
        }
    }
    run_systemctl(&["--user", "daemon-reload"])?;

    println!("Uninstalled {}", service_path.display());
    Ok(())
}

fn render_unit(exe: &Path, address: &BluetoothAddress, paths: &TrackerPaths) -> String {
    let exec_start = [
        systemd_arg(exe.as_os_str()),
        systemd_arg(OsStr::new("--log")),
        systemd_arg(paths.log_path.as_os_str()),
        systemd_arg(OsStr::new("--state")),
        systemd_arg(paths.state_path.as_os_str()),
        systemd_arg(OsStr::new("watch")),
        systemd_arg(OsStr::new("--address")),
        systemd_arg(OsStr::new(address.as_str())),
    ]
    .join(" ");

    format!(
        "[Unit]\n\
         Description=Keychron Bluetooth connection tracker\n\
         Documentation=man:systemd.service(5)\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec_start}\n\
         Restart=always\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to run systemctl {}", args.join(" ")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(anyhow!(
        "systemctl {} failed: {}{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn systemd_arg(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return value.into_owned();
    }

    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rendered_unit_runs_watch_with_paths_and_address() {
        let paths = TrackerPaths {
            log_path: PathBuf::from("/tmp/keychron spans.jsonl"),
            state_path: PathBuf::from("/tmp/keychron active.json"),
        };
        let unit = render_unit(
            Path::new("/tmp/keychron-tracker"),
            &BluetoothAddress::new("aa:bb:cc:dd:ee:ff"),
            &paths,
        );

        assert!(unit.contains("ExecStart=/tmp/keychron-tracker"));
        assert!(unit.contains("watch --address AA:BB:CC:DD:EE:FF"));
        assert!(unit.contains("\"/tmp/keychron spans.jsonl\""));
        assert!(unit.contains("Restart=always"));
    }
}
