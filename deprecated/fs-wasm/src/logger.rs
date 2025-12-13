use core::fmt::Write as FmtWrite;
use log::{Level, LevelFilter, Metadata, Record};

/// Simple logger that writes to stderr
struct StderrLogger;

static LOGGER: StderrLogger = StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Enable based on configured max level
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // Format: [LEVEL] target: message
        let level_str = match record.level() {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };

        // Use a stack-allocated buffer for the log message
        let mut buffer = ArrayString::<1024>::new();

        // Format the log message
        let _ = writeln!(
            buffer,
            "[{}] {}: {}",
            level_str,
            record.target(),
            record.args()
        );

        // Write to stderr using std::io
        use std::io::Write;
        let _ = std::io::stderr().write_all(buffer.as_bytes());
    }

    fn flush(&self) {
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
}

/// Initialize the logger with appropriate level based on build configuration
pub fn init() {
    // Set log level based on build configuration
    #[cfg(debug_assertions)]
    let level = LevelFilter::Debug;

    #[cfg(not(debug_assertions))]
    let level = LevelFilter::Info;

    // Set the logger
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
        log::info!("Logger initialized with level: {:?}", level);
    }
}

/// A fixed-capacity string buffer on the stack
struct ArrayString<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> ArrayString<N> {
    fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl<const N: usize> FmtWrite for ArrayString<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        if self.len + bytes.len() > N {
            return Err(core::fmt::Error);
        }
        self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}
