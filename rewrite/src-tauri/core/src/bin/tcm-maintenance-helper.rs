// No WebView, database configuration, media roots or network client in this process.
fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(tcm_core::maintenance_macos::main());
    #[cfg(target_os = "linux")]
    std::process::exit(tcm_core::maintenance_linux::main());
    #[cfg(windows)]
    std::process::exit(tcm_core::maintenance_windows::main());
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    std::process::exit(64);
}
