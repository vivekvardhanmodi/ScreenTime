pub mod kwin;
pub mod receiver;
pub mod script;
pub mod tracker;

pub use kwin::{load_and_run_kwin_script, unload_kwin_script, KWinScriptProxy, KWinScriptingProxy};
pub use receiver::{parse_window_changed, KdeTrackerReceiver};
pub use script::{generate_kwin_script, resolve_kwin_script_path, DEFAULT_KWIN_SCRIPT};
pub use tracker::KdeWindowTracker;
