pub mod db;
pub mod domain;
pub mod ipc;
pub mod rules;
pub mod traits;

pub use db::{apply_migrations_with_baseline, DbActorHandle, DbCommand, DbError, MIGRATION_1_SQL};
pub use domain::{extract_domain, IdleState, Session, TitleRule, WindowInfo};
pub use ipc::{
    default_runtime_dir, default_socket_name, default_socket_path, read_length_prefixed,
    socket_name_from_path, write_length_prefixed, BrowserExtensionMessage, BrowserIpcMessage,
    HostExtensionResponse, UrlUpdate, DEFAULT_SOCKET_NAME, DEFAULT_WINDOWS_PIPE_NAME,
    MAX_MESSAGE_SIZE,
};
pub use rules::{TargetRule, TitleRulesCache};
pub use traits::{IdleDetector, NoopIdleDetector, TrackerError, WindowTracker};
