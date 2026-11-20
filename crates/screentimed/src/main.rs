use screentimed::{init_logging, parse_cli_args, run_main, Config, RunOptions, Subcommand};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config = if let Ok(Subcommand::Run(ref opts)) = parse_cli_args(&args) {
        Config::load_from_env_and_cli(opts)
    } else {
        Config::load_from_env_and_cli(&RunOptions::default())
    };

    let _ = init_logging(&config);

    let exit_code = run_main(args).await;
    std::process::exit(exit_code);
}
