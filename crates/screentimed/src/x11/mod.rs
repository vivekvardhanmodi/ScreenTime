pub mod atoms;
pub mod common;
pub mod idle;
pub mod tracker;

pub use common::{
    clean_text_property, evaluate_idle_state, fetch_text_property, fetch_window_info,
    fetch_window_title, fetch_wm_class, is_ignorable_x11_error, parse_wm_class_bytes,
    query_root_active_window, unwatch_window, watch_window,
};
pub use idle::X11IdleDetector;
pub use tracker::X11WindowTracker;
