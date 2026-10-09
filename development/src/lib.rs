use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tokio::{
    process::Command,
    time::{Duration, sleep, timeout},
};

/// Variables to hand to the development processes, read from `.env` and then
/// `.env.local` (which wins). Both files are optional. `.env.local` is
/// gitignored, so it is where credentials that must not be committed belong,
/// such as the CSB security-key ones (`CSB_WEBAUTHN_ORIGIN`,
/// `CSB_WEBAUTHN_USERS`).
///
/// A variable already set in the caller's own environment is skipped, so
/// `DEFAULT_ELECTION=PS27:prov1 bin/dev` still wins over the file.
pub fn dotenv_variables() -> Result<Vec<(String, String)>> {
    let mut variables = BTreeMap::new();

    for file in [".env", ".env.local"] {
        let entries = match dotenvy::from_filename_iter(file) {
            Ok(entries) => entries,
            Err(error) if error.not_found() => continue,
            Err(error) => return Err(error).with_context(|| format!("read {file}")),
        };

        for entry in entries {
            let (name, value) = entry.with_context(|| format!("parse {file}"))?;
            variables.insert(name, value);
        }
    }

    Ok(variables
        .into_iter()
        .filter(|(name, _)| std::env::var_os(name).is_none())
        .collect())
}

pub async fn run(command: &str, args: &[&str]) -> Result<()> {
    println!("$> {command} {}", args.join(" "));
    let status = Command::new(command).args(args).status().await?;

    if !status.success() {
        anyhow::bail!("command failed: {:?}", command);
    }

    Ok(())
}

pub async fn stop_running_containers() -> Result<()> {
    let output = Command::new("docker").args(["ps", "-q"]).output().await?;
    if output.status.success() {
        let ids = String::from_utf8_lossy(&output.stdout);
        let ids: Vec<&str> = ids.split_whitespace().collect();
        if !ids.is_empty() {
            let mut args = Vec::with_capacity(ids.len() + 1);
            args.push("kill");
            args.extend(ids.iter().cloned());
            run("docker", &args).await?;
        }
    }
    Ok(())
}

pub async fn wait_for_postgres() -> Result<()> {
    for _ in 0..20 {
        let attempt = timeout(
            Duration::from_secs(1),
            Command::new("docker")
                .args(["compose", "exec", "-T", "psql", "pg_isready", "-U", "eks"])
                .status(),
        )
        .await;

        if matches!(attempt, Ok(Ok(status)) if status.success()) {
            println!("✅ PostgreSQL is up!");

            // small delay to ensure connecting from outside docker works
            sleep(Duration::from_millis(500)).await;

            return Ok(());
        }

        println!("⏳ Waiting for PostgreSQL...");
        sleep(Duration::from_secs(1)).await;
    }

    anyhow::bail!("PostgreSQL did not start in time");
}

pub fn pts(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("convert {path:?} to str"))
}

pub async fn platform_string() -> Result<String> {
    let output = Command::new("uname").args(["-ms"]).output().await?;

    if !output.status.success() {
        anyhow::bail!("uname -ms failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn temp_dir() -> Result<PathBuf> {
    let output = Command::new("mktemp").arg("-d").output().await?;

    if !output.status.success() {
        anyhow::bail!("mktemp -d failed");
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}
