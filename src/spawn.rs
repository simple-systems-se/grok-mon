use std::process::{Command, Stdio};

pub fn detach(cmd: &mut Command) -> &mut Command {
    cmd.env_remove("WAYLAND_SOCKET")
        .env_remove("X_PRIVILEGED_WAYLAND_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    detach(&mut cmd)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {program}: {e}"))
}
