use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use falcon_library::Library;
use falcon_library::import::ImportOptions;

#[derive(Parser)]
#[command(version, about = "Work with Eagle libraries")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import files into a library and print the new item ids
    Import {
        /// Path of the .library folder
        library: PathBuf,
        /// Files to import
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Folder id to put the items in (repeatable)
        #[arg(long = "folder")]
        folders: Vec<String>,
        /// Tag to add (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Skip palette extraction
        #[arg(long)]
        no_palette: bool,
    },
    /// Print item, folder and tag counts
    Info {
        /// Path of the .library folder
        library: PathBuf,
    },
}

fn run(cli: Cli) -> falcon_library::Result<()> {
    match cli.command {
        Command::Import { library, files, folders, tags, no_palette } => {
            let mut lib = Library::open(library)?;
            let opts = ImportOptions { folders, tags, palette: !no_palette, ..Default::default() };
            for file in files {
                println!("{}", lib.import_file(file, &opts)?);
            }
        }
        Command::Info { library } => {
            let lib = Library::open(library)?;
            let trashed = lib.items().iter().filter(|i| i.is_deleted).count();
            let mut tags: Vec<&str> = lib.items().iter().flat_map(|i| i.tags.iter().map(String::as_str)).collect();
            tags.sort_unstable();
            tags.dedup();
            println!("items:   {} ({trashed} in trash)", lib.items().len());
            println!("folders: {}", lib.all_folders().len());
            println!("tags:    {}", tags.len());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
