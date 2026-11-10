use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{fs, process::Command};

use eks_development::{
    platform_string, pts, run, stop_running_containers, temp_dir, wait_for_postgres,
};

const BIN_DIR: &str = "bin";

#[tokio::main]
async fn main() -> Result<()> {
    let platform = platform_string().await.context("detect platform")?;
    let config = load_config().await.context("load setup config")?;
    let bin_dir = Path::new(BIN_DIR);

    fs::create_dir_all(bin_dir)
        .await
        .context("create tools directory")?;

    // if an argument is passed, only install that tool
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 {
        let tool_name = &args[1];
        let Some(tool) = config.tools.iter().find(|t| &t.name == tool_name) else {
            anyhow::bail!("unknown tool: {}", tool_name);
        };
        tool.verify_installed(&platform, bin_dir).await?;

        return Ok(());
    }

    for tool in config.tools {
        tool.verify_installed(&platform, bin_dir).await?;
    }

    println!("🚀 Building djlint Docker container...");
    config.commands.build_djlint_docker_image.run().await?;

    println!("🚀 Setting up Docker containers...");
    stop_running_containers().await?;

    config.commands.docker_compose_rm.run().await?;
    config.commands.docker_compose_up.run().await?;

    println!("📦 Bundling frontend assets with esbuild...");
    config.commands.esbuild_bundle.run().await?;

    println!("📚 Installing cargo-watch (if it is not yet installed)...");
    config.commands.install_cargo_watch.run().await?;

    println!("📚 Installing cargo-dylint (if it is not yet installed)...");
    config.commands.install_dylint.run().await?;

    wait_for_postgres().await?;

    println!("✅ Yay, setup complete!");

    println!("🔨 You can run 'bin/dev' to start the development environment.");

    Ok(())
}

#[derive(Deserialize)]
struct ToolConfig {
    name: String,
    version: String,
    base_url: String,
    /// sha256 of the download, keyed by platform suffix
    sha256: HashMap<String, String>,
}

#[derive(Deserialize)]
struct CommandConfig {
    command: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
struct CommandsConfig {
    docker_compose_rm: CommandConfig,
    docker_compose_up: CommandConfig,
    build_djlint_docker_image: CommandConfig,
    install_cargo_watch: CommandConfig,
    install_dylint: CommandConfig,
    esbuild_bundle: CommandConfig,
}

#[derive(Deserialize)]
struct SetupConfig {
    tools: Vec<ToolConfig>,
    commands: CommandsConfig,
}

async fn load_config() -> Result<SetupConfig> {
    let contents = include_str!("../../setup.yml");
    let config: SetupConfig = serde_saphyr::from_str(contents).context("parse setup.yml")?;

    Ok(config)
}

impl CommandConfig {
    async fn run(&self) -> Result<()> {
        // replace __UID__ and __GID__ in args with the current user's UID and GID
        let uid: u16 = std::env::var("UID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);
        let gid: u16 = std::env::var("GID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| {
                arg.replace("__UID__", &uid.to_string())
                    .replace("__GID__", &gid.to_string())
            })
            .collect();

        let status = Command::new(&self.command).args(&args).status().await?;

        if !status.success() {
            anyhow::bail!("command failed: {:?}", self.command);
        }

        Ok(())
    }
}

impl ToolConfig {
    async fn verify_installed(&self, platform: &str, bin_dir: &Path) -> Result<()> {
        let target = bin_dir.join(&self.name);

        if fs::try_exists(&target).await? {
            println!("✅ {} already installed", self.name);

            let output = Command::new(&target)
                .arg("--version")
                .output()
                .await
                .with_context(|| format!("check version of installed {}", self.name))?;

            let installed_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !installed_version.contains(&self.version) {
                println!(
                    "⚠️  {} version mismatch: installed {}, expected {}",
                    self.name, installed_version, self.version
                );
                self.install(platform, &target)
                    .await
                    .with_context(|| format!("re-install {}", self.name))?;

                println!("✅ {} updated to version {}", self.name, self.version);
            }
        } else {
            println!("📦 Installing {} for platform: {platform}", self.name);
            self.install(platform, &target)
                .await
                .with_context(|| format!("install {}", self.name))?;

            println!("✅ {} installed", self.name);
        }

        Ok(())
    }

    async fn verify_sha256(&self, platform_suffix: &str, file: &Path) -> Result<()> {
        let expected = self.sha256.get(platform_suffix).with_context(|| {
            format!("no sha256 for {} {platform_suffix} in setup.yml", self.name)
        })?;
        let bytes = fs::read(file)
            .await
            .with_context(|| format!("read downloaded {}", self.name))?;
        let actual: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        if &actual != expected {
            anyhow::bail!(
                "sha256 mismatch for {} {platform_suffix}: expected {expected}, got {actual}",
                self.name
            );
        }

        Ok(())
    }

    async fn install(&self, platform: &str, target: &Path) -> Result<()> {
        match self.name.as_str() {
            "esbuild" => install_esbuild(platform, target, self).await,
            "biome" => install_biome(platform, target, self).await,
            _ => anyhow::bail!("unknown tool: {}", self.name),
        }
    }
}

async fn install_esbuild(platform: &str, target: &Path, tool: &ToolConfig) -> Result<()> {
    let temp_dir = temp_dir().await.context("create temp dir")?;
    let temp_esbuild = temp_dir.join(format!("esbuild-{}.tgz", tool.version));
    let platform_suffix = platform_suffix(platform, &ESBUILD_SUFFIXES)?;
    let url = format!(
        "{}/{}/-/{platform_suffix}-{}.tgz",
        tool.base_url, platform_suffix, tool.version
    );

    run("curl", &["-fo", pts(&temp_esbuild)?, &url]).await?;
    tool.verify_sha256(platform_suffix, &temp_esbuild).await?;

    println!("📂 Extracting esbuild...");
    run(
        "tar",
        &[
            "-xzf",
            pts(&temp_esbuild)?,
            "-C",
            pts(&temp_dir)?,
            "package/bin/esbuild",
        ],
    )
    .await?;

    let from = temp_dir.join("package/bin/esbuild");
    fs::copy(&from, target)
        .await
        .context("move esbuild into tools directory")?;
    fs::remove_dir_all(&temp_dir)
        .await
        .context("remove temporary directory")?;

    Ok(())
}

async fn install_biome(platform: &str, target: &Path, tool: &ToolConfig) -> Result<()> {
    let platform_suffix = platform_suffix(platform, &BIOME_SUFFIXES)?;
    let url = format!("{}{}/{}", tool.base_url, tool.version, platform_suffix);

    run("curl", &["-Lfo", pts(target)?, &url])
        .await
        .with_context(|| format!("download {}", tool.name))?;
    if let Err(e) = tool.verify_sha256(platform_suffix, target).await {
        // never leave an unverified binary behind
        fs::remove_file(target).await.ok();
        return Err(e);
    }
    run("chmod", &["+x", pts(target)?])
        .await
        .with_context(|| format!("mark {} as executable", tool.name))
}

const ESBUILD_SUFFIXES: [(&str, &str); 4] = [
    ("Darwin arm64", "darwin-arm64"),
    ("Linux arm64", "linux-arm64"),
    ("Linux aarch64", "linux-arm64"),
    ("Linux x86_64", "linux-x64"),
];

const BIOME_SUFFIXES: [(&str, &str); 4] = [
    ("Darwin arm64", "biome-darwin-arm64"),
    ("Linux arm64", "biome-linux-arm64-musl"),
    ("Linux aarch64", "biome-linux-arm64-musl"),
    ("Linux x86_64", "biome-linux-x64-musl"),
];

fn platform_suffix<'a>(platform: &str, suffixes: &'a [(&str, &'a str)]) -> Result<&'a str> {
    suffixes
        .iter()
        .find_map(|(key, suffix)| (*key == platform).then_some(*suffix))
        .ok_or_else(|| anyhow::anyhow!("unsupported platform: {platform}"))
}
