//! halycon: CLI tool for Halycon VFS
//!
//! Embed files, compose WASM components, and extract bundled binaries.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod compose;
mod embed;
mod extract;
mod wasm;

#[derive(Parser, Debug)]
#[command(name = "halycon")]
#[command(
    about = "CLI tool for Halycon VFS - embed files, compose WASM components, and extract binaries"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Embed files into the bundled vfs-adapter
    Embed {
        /// Output WASM file
        #[arg(short, long, required = true)]
        output: PathBuf,

        /// Mount a local directory into the virtual filesystem
        /// Format: /virtual-path=./local-path
        #[arg(short, long = "mount", value_name = "MOUNT", required = true)]
        mounts: Vec<String>,

        /// Use the S3 sync variant of vfs-adapter
        #[arg(long)]
        s3_sync: bool,
    },

    /// Compose an app with a bundled adapter
    Compose {
        /// Input app WASM file
        #[arg(required = true)]
        app: PathBuf,

        /// Output composed WASM file
        #[arg(short, long, required = true)]
        output: PathBuf,

        /// Use rpc-adapter instead of vfs-adapter
        #[arg(long)]
        rpc: bool,

        /// Use the S3 sync variant
        #[arg(long)]
        s3_sync: bool,

        /// Embed files into the adapter before composing
        /// Format: /virtual-path=./local-path
        #[arg(short, long = "mount", value_name = "MOUNT")]
        mounts: Vec<String>,
    },

    /// Extract a bundled WASM binary to a file
    Extract {
        /// Binary to extract: adapter, rpc-adapter, server
        #[arg(required = true)]
        name: String,

        /// Output file path
        #[arg(short, long, required = true)]
        output: PathBuf,

        /// Use the S3 sync variant
        #[arg(long)]
        s3_sync: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Embed {
            output,
            mounts,
            s3_sync,
        } => {
            embed::run(&output, &mounts, s3_sync)?;
        }

        Commands::Compose {
            app,
            output,
            rpc,
            s3_sync,
            mounts,
        } => {
            compose::run(&app, &output, rpc, s3_sync, &mounts)?;
        }

        Commands::Extract {
            name,
            output,
            s3_sync,
        } => {
            extract::run(&name, &output, s3_sync)?;
        }
    }

    Ok(())
}
