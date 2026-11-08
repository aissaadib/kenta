fn main() {
  #[cfg(windows)]
  {
    // tauri-build compiles its icon/version resource only into binary targets.
    // Rust's lib-test executable therefore used to have no application
    // manifest, so common-controls v6 was never activated. `muda` (a Tauri
    // dependency) imports `TaskDialogIndirect` from comctl32, which exists only
    // in v6, and the test binary aborted at load with
    // STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // Embed a manifest-only resource into every linkable target and tell
    // tauri-build not to embed its own copy, so each executable still contains
    // exactly one manifest. A distinct .rc filename keeps the two compiled
    // resources in separate files instead of overwriting tauri-build's output.
    let _ = embed_resource::compile_for_everything("app-manifest.rc", embed_resource::NONE);

    let attributes = tauri_build::Attributes::default().windows_attributes(
      tauri_build::WindowsAttributes::new_without_app_manifest(),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");

    // Cargo runs test executables with the crate root as the working directory,
    // and Windows only searches the executable's own directory and PATH for a
    // DLL. tauri-build places WebView2Loader.dll beside the app binary, so copy
    // it beside the test binaries too.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
      if let Some(profile_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) {
        let source = profile_dir.join("WebView2Loader.dll");
        let deps = profile_dir.join("deps");
        if source.is_file() && deps.is_dir() {
          let _ = std::fs::copy(source, deps.join("WebView2Loader.dll"));
        }
      }
    }
  }

  #[cfg(not(windows))]
  tauri_build::build()
}
