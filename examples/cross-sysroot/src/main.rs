fn main() {
    println!("Hello from cross-sysroot example!");
    println!("arch:     {}", std::env::consts::ARCH);
    println!("os:       {}", std::env::consts::OS);

    if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
        println!("hostname: {}", hostname.trim());
    }
}
