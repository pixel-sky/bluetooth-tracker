use crate::{
    address::BluetoothAddress,
    paths::{TrackerPaths, default_user_systemd_dir},
};
use anyhow::{Context, Result, anyhow};
use std::{env, ffi::OsStr, fs, path::Path, process::Command};

const SERVICE_NAME: &str = "keychron-tracker.service";

pub fn install(addresses: impl AsRef<[BluetoothAddress]>, paths: &TrackerPaths) -> Result<()> {
    let service_dir = default_user_systemd_dir()?;
    fs::create_dir_all(&service_dir)
        .with_context(|| format!("failed to create {}", service_dir.display()))?;

    let service_path = service_dir.join(SERVICE_NAME);
    let exe = env::current_exe().context("failed to locate current executable")?;
    let unit = render_unit(&exe, addresses, paths)?;
    fs::write(&service_path, unit)
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    run_systemctl(["--user", "daemon-reload"])?;
    run_systemctl(["--user", "enable", "--now", SERVICE_NAME])?;

    println!("Installed {}", service_path.display());
    println!("Status: systemctl --user status keychron-tracker");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let service_dir = default_user_systemd_dir()?;
    let service_path = service_dir.join(SERVICE_NAME);

    run_systemctl(["--user", "disable", "--now", SERVICE_NAME])?;
    match fs::remove_file(&service_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("Service not found; possibly already uninstalled");
            return Ok(());
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to remove {}", service_path.display()));
        }
    }
    run_systemctl(["--user", "daemon-reload"])?;

    println!("Uninstalled {}", service_path.display());
    Ok(())
}

fn render_unit(
    exe: impl AsRef<Path>,
    addresses: impl AsRef<[BluetoothAddress]>,
    paths: &TrackerPaths,
) -> Result<String> {
    let state_dir = std::path::absolute(paths.state_dir()).with_context(|| {
        format!(
            "failed to resolve state directory {}",
            paths.state_dir().display()
        )
    })?;
    let mut args = vec![
        systemd_arg(exe.as_ref().as_os_str()),
        systemd_arg(OsStr::new("--state-dir")),
        systemd_arg(state_dir.as_os_str()),
        systemd_arg(OsStr::new("watch")),
    ];
    for address in addresses.as_ref() {
        args.push(systemd_arg(OsStr::new("--address")));
        args.push(systemd_arg(OsStr::new(address.as_str())));
    }
    let exec_start = args.join(" ");

    Ok(format!(
        "[Unit]\n\
         Description=Bluetooth device connection tracker\n\
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
    ))
}

fn run_systemctl(args: impl AsRef<[&'static str]>) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args.as_ref())
        .output()
        .with_context(|| format!("failed to run systemctl {}", args.as_ref().join(" ")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(anyhow!(
        "systemctl {} failed: {}{}",
        args.as_ref().join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn systemd_arg(value: impl AsRef<OsStr>) -> String {
    let value = value.as_ref().to_string_lossy();
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
    #[test]
    fn rendered_unit_runs_watch_with_paths_and_address() {
        let paths = TrackerPaths::new("/tmp/keychron state");
        let unit = render_unit(
            Path::new("/tmp/keychron-tracker"),
            &[
                BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff"),
                BluetoothAddress::new_unchecked("11:22:33:44:55:66"),
            ],
            &paths,
        )
        .unwrap();

        assert!(unit.contains(
            "ExecStart=/tmp/keychron-tracker --state-dir \"/tmp/keychron state\" watch \
             --address AA:BB:CC:DD:EE:FF"
        ));
        assert!(unit.contains("--address 11:22:33:44:55:66"));
        assert!(!unit.contains("--log"));
        assert!(!unit.contains("--state "));
        assert!(unit.contains("Restart=always"));
    }

    #[test]
    fn rendered_unit_resolves_relative_state_directory() {
        let paths = TrackerPaths::new("relative keychron state");
        let unit = render_unit(Path::new("/tmp/keychron-tracker"), [], &paths).unwrap();
        let state_dir = env::current_dir().unwrap().join("relative keychron state");

        assert!(unit.contains(&format!(
            "--state-dir {} watch",
            systemd_arg(state_dir.as_os_str())
        )));
    }
}
