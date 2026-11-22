use screentimed::run_main;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exit_code = run_main(args).await;
    std::process::exit(exit_code);
}
