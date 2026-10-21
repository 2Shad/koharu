fn main() {
    // No JS commands: the plugin only exposes a Rust API to the app.
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .build();
}
