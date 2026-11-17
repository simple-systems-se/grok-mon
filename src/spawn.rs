use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    spawn_detached_cmd(&mut cmd, program)
}

pub fn spawn_detached_cmd(cmd: &mut Command, name: &str) -> Result<(), String> {
    let mut child = detach(cmd)
        .spawn()
        .map_err(|e| format!("failed to launch {name}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub enum QuietStop {
    Exited(ExitStatus),
    TimedOut,
}

/// Run `cmd` with no TTY, wait up to `timeout`, then stop the process group.
///
/// Stdin, stdout, and stderr are null, so none of them is a terminal. The
/// child is placed in its own process group and that group is killed on
/// timeout. This does not change the current process environment.
pub fn run_quiet(cmd: &mut Command, timeout: Duration) -> Result<QuietStop, std::io::Error> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(QuietStop::Exited(status)),
            Ok(None) if started.elapsed() >= timeout => {
                stop_child(&mut child);
                return Ok(QuietStop::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                stop_child(&mut child);
                return Err(err);
            }
        }
    }
}

fn stop_child(child: &mut Child) {
    kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn kill_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}
