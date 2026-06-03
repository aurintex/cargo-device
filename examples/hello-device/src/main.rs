fn main() {
    println!("Hello from cargo-device!");
    println!("arch:     {}", std::env::consts::ARCH);
    println!("os:       {}", std::env::consts::OS);

    if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
        println!("hostname: {}", hostname.trim());
    }

    if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
        // Field name varies by SoC: "Model", "model name", "cpu model", "Hardware"
        if let Some(line) = info.lines().find(|l| {
            let key = l.split(':').next().unwrap_or("").to_ascii_lowercase();
            key.contains("model") || key.trim() == "hardware"
        })
        {
            println!("{line}");
        }
    }
}
