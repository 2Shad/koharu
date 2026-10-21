fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "android" | "ios") {
        println!("cargo::rustc-cfg=mobile");
    } else {
        println!("cargo::rustc-cfg=desktop");
    }
}
