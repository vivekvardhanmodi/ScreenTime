pub mod dispatch;
pub mod idle;
pub mod toplevel;

pub use dispatch::WaylandConnectionSource;
pub use idle::{compute_idle_state, WaylandIdleDetector, WaylandIdleState};
pub use toplevel::{
    compute_active_window, parse_state_activated, ToplevelData, WlrForeignToplevelState,
    WlrForeignToplevelTracker,
};
