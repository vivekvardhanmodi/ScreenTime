use screentimed::run_main;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    let exit_code = run_main(args).await;
    std::process::exit(exit_code);
}
