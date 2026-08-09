//! Quick Tunnel через `cloudflared` — бинарь в PATH или Docker-образ.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use regex_lite::Regex;

/// Держит child `cloudflared` / `docker run` и убивает на Drop.
pub struct TunnelGuard {
    child: Child,
    /// Public HTTPS origin, e.g. `https://foo-bar.trycloudflare.com`.
    pub public_url: String,
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether tunnel mode was requested via env / CLI.
pub fn tunnel_requested() -> bool {
    if std::env::args().any(|a| a == "--tunnel" || a == "-t") {
        return true;
    }
    matches!(
        std::env::var("PASSKEY_TUNNEL").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    ) || matches!(
        std::env::var("CLOUDFLARED_TUNNEL").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

fn prefer_docker_cloudflared() -> bool {
    matches!(
        std::env::var("CLOUDFLARED_DOCKER").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    ) || which::which("cloudflared").is_err()
}

enum CloudflaredLaunch {
    Binary { path: String, args: Vec<String> },
    Docker { args: Vec<String> },
}

fn launch_spec(local: SocketAddr) -> Result<CloudflaredLaunch, String> {
    let target_local = format!("http://{local}");
    // Docker → хост: на Mac/Win host.docker.internal; на Linux тоже через extra_hosts.
    let host_port = local.port();
    let target_docker = format!("http://host.docker.internal:{host_port}");

    if let Ok(p) = std::env::var("CLOUDFLARED_BIN") {
        return Ok(CloudflaredLaunch::Binary {
            path: p,
            args: vec![
                "tunnel".into(),
                "--no-autoupdate".into(),
                "--url".into(),
                target_local,
            ],
        });
    }

    if prefer_docker_cloudflared() {
        which::which("docker").map_err(|_| {
            "нужен docker (cloudflared CLI нет в PATH). Поставь Docker Desktop / docker engine"
                .to_string()
        })?;
        let image = std::env::var("CLOUDFLARED_IMAGE")
            .unwrap_or_else(|_| "cloudflare/cloudflared:latest".into());
        return Ok(CloudflaredLaunch::Docker {
            args: vec![
                "run".into(),
                "--rm".into(),
                "-i".into(),
                "--add-host".into(),
                "host.docker.internal:host-gateway".into(),
                image,
                "tunnel".into(),
                "--no-autoupdate".into(),
                "--url".into(),
                target_docker,
            ],
        });
    }

    let path = which::which("cloudflared")
        .map(|p| p.display().to_string())
        .map_err(|_| "cloudflared не найден".to_string())?;
    Ok(CloudflaredLaunch::Binary {
        path,
        args: vec![
            "tunnel".into(),
            "--no-autoupdate".into(),
            "--url".into(),
            target_local,
        ],
    })
}

/// Стартует cloudflared (бинарь или docker), ждёт trycloudflare URL.
pub fn start_quick_tunnel(local: SocketAddr) -> Result<TunnelGuard, String> {
    let spec = launch_spec(local)?;

    let mut child = match &spec {
        CloudflaredLaunch::Binary { path, args } => {
            tracing::info!(cloudflared = %path, "поднимаем Cloudflare Quick Tunnel (binary)");
            Command::new(path)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("не удалось запустить cloudflared: {e}"))?
        }
        CloudflaredLaunch::Docker { args } => {
            tracing::info!("поднимаем Cloudflare Quick Tunnel (docker cloudflared)");
            Command::new("docker")
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("не удалось запустить docker cloudflared: {e}"))?
        }
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cloudflared: нет stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cloudflared: нет stderr".to_string())?;

    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    let tx_err = tx.clone();

    let re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")
        .map_err(|e| e.to_string())?;

    let pump = |reader: Box<dyn BufRead + Send>,
                tx: mpsc::Sender<Result<String, String>>,
                re: Regex,
                label: &'static str| {
        thread::spawn(move || {
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        tracing::debug!(target: "cloudflared", "{label}: {line}");
                        if let Some(m) = re.find(&line) {
                            let _ = tx.send(Ok(m.as_str().trim_end_matches('/').to_string()));
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(format!("cloudflared {label}: {e}")));
                        return;
                    }
                }
            }
        });
    };

    pump(Box::new(BufReader::new(stdout)), tx, re.clone(), "stdout");
    pump(Box::new(BufReader::new(stderr)), tx_err, re, "stderr");

    let public_url = rx
        .recv_timeout(Duration::from_secs(60))
        .map_err(|_| "timeout: cloudflared не отдал trycloudflare.com URL за 60s".to_string())??;

    Ok(TunnelGuard { child, public_url })
}

/// Host + origin from a public HTTPS tunnel URL.
pub fn host_and_origin(public_url: &str) -> Result<(String, String), String> {
    let url = url::Url::parse(public_url).map_err(|e| format!("bad tunnel url: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "tunnel url without host".to_string())?
        .to_string();
    Ok((host.clone(), format!("https://{host}")))
}
