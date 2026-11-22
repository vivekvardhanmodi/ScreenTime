use crate::config::Config;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

pub fn strip_ansi(input: &[u8]) -> Vec<u8> {
    if !input.contains(&0x1B) {
        return input.to_vec();
    }

    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x1B {
            i += 1;
            if i < input.len() && input[i] == b'[' {
                i += 1;
                while i < input.len() {
                    let b = input[i];
                    i += 1;
                    if (0x40..=0x7E).contains(&b) {
                        break;
                    }
                }
            } else if i < input.len() && input[i] == b']' {
                i += 1;
                while i < input.len() {
                    if input[i] == 0x07 {
                        i += 1;
                        break;
                    } else if input[i] == 0x1B && i + 1 < input.len() && input[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else if i < input.len()
                && (input[i] == b'(' || input[i] == b')' || input[i] == b'#' || input[i] == b'%')
            {
                i = (i + 2).min(input.len());
            } else if i < input.len() {
                i += 1;
            }
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

pub fn backup_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

pub struct RollingFileWriter {
    path: PathBuf,
    max_bytes: u64,
    backup_count: usize,
    current_size: u64,
    file: Option<File>,
}

impl RollingFileWriter {
    pub fn new(path: impl Into<PathBuf>, max_bytes: u64, backup_count: usize) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let current_size = if path.exists() {
            fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            path,
            max_bytes,
            backup_count,
            current_size,
            file: Some(file),
        })
    }

    pub fn current_size(&self) -> u64 {
        self.current_size
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn backup_count(&self) -> usize {
        self.backup_count
    }

    pub fn rotate(&mut self) -> io::Result<()> {
        self.file = None;

        if self.backup_count > 0 {
            let oldest = backup_path(&self.path, self.backup_count);
            if oldest.exists() {
                let _ = fs::remove_file(&oldest);
            }

            for i in (1..self.backup_count).rev() {
                let src = backup_path(&self.path, i);
                let dst = backup_path(&self.path, i + 1);
                if src.exists() {
                    if dst.exists() {
                        let _ = fs::remove_file(&dst);
                    }
                    let _ = fs::rename(&src, &dst);
                }
            }

            if self.path.exists() {
                let dst = backup_path(&self.path, 1);
                if dst.exists() {
                    let _ = fs::remove_file(&dst);
                }
                let _ = fs::rename(&self.path, &dst);
            }
        } else if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }

        let new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;

        self.file = Some(new_file);
        self.current_size = 0;
        Ok(())
    }
}

impl Write for RollingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.max_bytes > 0
            && self.current_size > 0
            && self.current_size + (buf.len() as u64) > self.max_bytes
        {
            self.rotate()?;
        }

        let file = match self.file.as_mut() {
            Some(f) => f,
            None => {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?;
                self.file.insert(f)
            }
        };

        file.write_all(buf)?;
        self.current_size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(ref mut f) = self.file {
            f.flush()?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ScreenTimeLogWriter {
    file: Option<Arc<Mutex<RollingFileWriter>>>,
    console: bool,
}

impl ScreenTimeLogWriter {
    pub fn new(file: Option<Arc<Mutex<RollingFileWriter>>>, console: bool) -> Self {
        Self { file, console }
    }
}

impl Write for ScreenTimeLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.console {
            let _ = io::stderr().write_all(buf);
        }

        if let Some(ref file_arc) = self.file {
            let stripped = strip_ansi(buf);
            if let Ok(mut writer) = file_arc.lock() {
                let _ = writer.write_all(&stripped);
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.console {
            let _ = io::stderr().flush();
        }

        if let Some(ref file_arc) = self.file {
            if let Ok(mut writer) = file_arc.lock() {
                let _ = writer.flush();
            }
        }

        Ok(())
    }
}

pub fn init_logging(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let log_dir = &config.paths.log_dir;
    let _ = fs::create_dir_all(log_dir);

    let max_bytes = config.logging.max_size_mb.saturating_mul(1024 * 1024);
    let log_file_path = log_dir.join("screentimed.log");

    let rolling_writer =
        RollingFileWriter::new(log_file_path, max_bytes, config.logging.backup_count)
            .ok()
            .map(|w| Arc::new(Mutex::new(w)));

    let filter =
        EnvFilter::try_new(&config.logging.level).unwrap_or_else(|_| EnvFilter::new("info"));

    let log_writer = ScreenTimeLogWriter::new(rolling_writer, config.logging.console);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(true)
        .with_writer(move || log_writer.clone())
        .try_init();

    Ok(())
}
