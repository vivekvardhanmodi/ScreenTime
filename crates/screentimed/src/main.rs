use screentimed::{parse_cli_args, run_main, Config, RunOptions, Subcommand};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

struct DualWriter {
    file: Option<Arc<Mutex<std::fs::File>>>,
}

impl Write for DualWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write(buf);
        if let Some(ref file_arc) = self.file {
            if let Ok(mut f) = file_arc.lock() {
                let _ = f.write(buf);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(ref file_arc) = self.file {
            if let Ok(mut f) = file_arc.lock() {
                let _ = f.flush();
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config = if let Ok(Subcommand::Run(ref opts)) = parse_cli_args(&args) {
        Config::load_from_env_and_cli(opts)
    } else {
        Config::load_from_env_and_cli(&RunOptions::default())
    };

    let log_dir = &config.paths.log_dir;
    let _ = std::fs::create_dir_all(log_dir);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("screentimed.log"))
        .ok()
        .map(|f| Arc::new(Mutex::new(f)));

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || DualWriter {
            file: log_file.clone(),
        })
        .with_ansi(false)
        .try_init();

    let exit_code = run_main(args).await;
    std::process::exit(exit_code);
}
