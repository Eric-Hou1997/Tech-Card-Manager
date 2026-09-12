// No WebView, database configuration, media roots or network client in this process.
fn main() {
    #[cfg(target_os = "linux")]
    std::process::exit(tcm_core::maintenance_linux::main());
    #[cfg(not(target_os = "linux"))]
    std::process::exit(64);
}
