use screentime_browser_host::{resolve_socket_name, run_host};
use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    tracing_subscriber::fmt().with_writer(io::stderr).init();

    let socket_name = resolve_socket_name()?;
    let stdin = io::stdin();
    let stdout = io::stdout();

    let (res, _) = run_host(stdin, stdout, socket_name).await;
    res
}
