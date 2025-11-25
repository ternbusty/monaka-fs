// CLI Stub - Dummy wasi:cli implementation that discards output
//
// This component provides stub implementations of wasi:cli interfaces
// to avoid wasi:io/streams import conflicts in vfs-provider

#![allow(warnings)]

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "cli-stub",
    path: "../wit",
    generate_all,
});

// Export the component
export!(CliStub);

struct CliStub;

// Implement wasi:cli/stdout
impl exports::wasi::cli::stdout::Guest for CliStub {
    fn get_stdout() -> exports::wasi::cli::stdout::OutputStream {
        // Return a dummy OutputStream that discards all data
        exports::wasi::cli::stdout::OutputStream::new(DummyOutputStream)
    }
}

// Implement wasi:cli/stderr
impl exports::wasi::cli::stderr::Guest for CliStub {
    fn get_stderr() -> exports::wasi::cli::stderr::OutputStream {
        // Return a dummy OutputStream that discards all data
        exports::wasi::cli::stderr::OutputStream::new(DummyOutputStream)
    }
}

// Implement wasi:cli/stdin
impl exports::wasi::cli::stdin::Guest for CliStub {
    fn get_stdin() -> exports::wasi::cli::stdin::InputStream {
        // Return a dummy InputStream that returns EOF
        exports::wasi::cli::stdin::InputStream::new(DummyInputStream)
    }
}

// Implement wasi:cli/terminal-stdout
impl exports::wasi::cli::terminal_stdout::Guest for CliStub {
    fn get_terminal_stdout() -> Option<exports::wasi::cli::terminal_stdout::TerminalOutput> {
        None
    }
}

// Implement wasi:cli/terminal-stderr
impl exports::wasi::cli::terminal_stderr::Guest for CliStub {
    fn get_terminal_stderr() -> Option<exports::wasi::cli::terminal_stderr::TerminalOutput> {
        None
    }
}

// Implement wasi:cli/terminal-stdin
impl exports::wasi::cli::terminal_stdin::Guest for CliStub {
    fn get_terminal_stdin() -> Option<exports::wasi::cli::terminal_stdin::TerminalInput> {
        None
    }
}

// Dummy OutputStream implementation - discards all writes
struct DummyOutputStream;

impl exports::wasi::cli::stdout::GuestOutputStream for DummyOutputStream {
    fn check_write(&self) -> Result<u64, wasi::io::streams::StreamError> {
        Ok(u64::MAX) // Always ready to "write" (discard)
    }

    fn write(&self, _contents: Vec<u8>) -> Result<(), wasi::io::streams::StreamError> {
        // Discard the data
        Ok(())
    }

    fn blocking_write_and_flush(&self, _contents: Vec<u8>) -> Result<(), wasi::io::streams::StreamError> {
        Ok(())
    }

    fn flush(&self) -> Result<(), wasi::io::streams::StreamError> {
        Ok(())
    }

    fn blocking_flush(&self) -> Result<(), wasi::io::streams::StreamError> {
        Ok(())
    }

    fn subscribe(&self) -> wasi::io::poll::Pollable {
        // Always ready
        wasi::io::poll::Pollable::new(AlwaysReadyPollable)
    }

    fn write_zeroes(&self, _len: u64) -> Result<(), wasi::io::streams::StreamError> {
        Ok(())
    }

    fn blocking_write_zeroes_and_flush(&self, _len: u64) -> Result<(), wasi::io::streams::StreamError> {
        Ok(())
    }

    fn splice(&self, _src: wasi::io::streams::InputStreamBorrow<'_>, _len: u64) -> Result<u64, wasi::io::streams::StreamError> {
        Err(wasi::io::streams::StreamError::Closed)
    }

    fn blocking_splice(&self, _src: wasi::io::streams::InputStreamBorrow<'_>, _len: u64) -> Result<u64, wasi::io::streams::StreamError> {
        Err(wasi::io::streams::StreamError::Closed)
    }
}

// Dummy InputStream implementation - always returns EOF
struct DummyInputStream;

impl exports::wasi::cli::stdin::GuestInputStream for DummyInputStream {
    fn read(&self, _len: u64) -> Result<Vec<u8>, wasi::io::streams::StreamError> {
        // Return empty buffer (EOF)
        Ok(Vec::new())
    }

    fn blocking_read(&self, _len: u64) -> Result<Vec<u8>, wasi::io::streams::StreamError> {
        Ok(Vec::new())
    }

    fn skip(&self, _len: u64) -> Result<u64, wasi::io::streams::StreamError> {
        Ok(0)
    }

    fn blocking_skip(&self, _len: u64) -> Result<u64, wasi::io::streams::StreamError> {
        Ok(0)
    }

    fn subscribe(&self) -> wasi::io::poll::Pollable {
        wasi::io::poll::Pollable::new(AlwaysReadyPollable)
    }
}

// Always-ready Pollable implementation
struct AlwaysReadyPollable;

impl wasi::io::poll::GuestPollable for AlwaysReadyPollable {
    fn ready(&self) -> bool {
        true
    }

    fn block(&self) {
        // No-op
    }
}

// Implement Guest trait for wasi:io/poll (for Pollable resource)
impl wasi::io::poll::Guest for CliStub {
    type Pollable = AlwaysReadyPollable;

    fn poll(_pollables: Vec<wasi::io::poll::PollableBorrow<'_>>) -> Vec<u32> {
        // All are ready
        (0.._pollables.len() as u32).collect()
    }
}
