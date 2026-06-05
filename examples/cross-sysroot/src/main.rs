use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Hello from cross-sysroot example!");
    tracing::info!(arch = std::env::consts::ARCH, os = std::env::consts::OS);

    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|h| h.trim().to_owned())
        .unwrap_or_else(|_| "<unknown>".to_owned());
    tracing::info!(hostname);

    Ok(())
}
