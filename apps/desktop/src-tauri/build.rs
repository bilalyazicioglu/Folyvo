fn main() {
    // The webview assets must exist at compile time. A fresh checkout has not built the UI yet.
    let _ = std::fs::create_dir_all("../ui/dist");
    tauri_build::build()
}
