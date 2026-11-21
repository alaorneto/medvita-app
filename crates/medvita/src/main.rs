//! Medvita Desktop.
// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use medvita_platform::{AppPaths, build_info};

fn main() {
    // Must run first: during install, update and uninstall Velopack runs its hooks here and may
    // exit the process. It also applies a downloaded update on startup. The AppUserModelID stays
    // Velopack's (`velopack.Medvita`, from the package manifest): the Start menu and desktop
    // shortcuts carry the same one, so the taskbar groups and pins the running app with them.
    velopack::VelopackApp::build().run();

    let paths = AppPaths::discover()
        .unwrap_or_else(|| AppPaths::under(&std::env::temp_dir().join(build_info::DIR_NAME)));
    if let Err(error) = paths.ensure() {
        eprintln!("could not create the app directories: {error}");
    }
    let _logging = medvita_platform::logging::init(&paths.logs_dir());
    tracing::info!(
        version = build_info::version(),
        commit = build_info::commit().unwrap_or("-"),
        "Medvita starting"
    );

    let runtime = medvita_ui::bridge::new_runtime().expect("could not start the async runtime");
    let secrets = medvita_auth::default_store();
    medvita_ui::run(medvita_ui::Boot { paths, secrets }, runtime);
    tracing::info!("Medvita closed");
}
