use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use falcon_library::Library;
use falcon_library::import::ImportOptions;
use falcon_library::pack::{PackImportOptions, PackSource, export_pack, import_pack};

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
    /// Create an empty library
    New {
        /// Path of the new .library folder
        library: PathBuf,
    },
    /// Create a folder and print its id
    Folder {
        /// Path of the .library folder
        library: PathBuf,
        /// Folder name
        name: String,
        /// Parent folder id
        #[arg(long)]
        parent: Option<String>,
    },
    /// Print item, folder and tag counts
    Info {
        /// Path of the .library folder
        library: PathBuf,
    },
    /// Back up metadata.json into the library's backup folder, as Eagle does
    Backup {
        /// Path of the .library folder
        library: PathBuf,
    },
    /// Export a folder or items as an .eaglepack
    Export {
        /// Path of the .library folder
        library: PathBuf,
        /// Pack file to write
        out: PathBuf,
        /// Export this folder and its subfolders
        #[arg(long, conflicts_with = "items", required_unless_present = "items")]
        folder: Option<String>,
        /// Item ids to export
        items: Vec<String>,
    },
    /// Import an .eaglepack and print the new item ids
    ImportPack {
        /// Path of the .library folder
        library: PathBuf,
        /// The .eaglepack file
        pack: PathBuf,
        /// Put the items in this folder (packs of items only)
        #[arg(long)]
        folder: Option<String>,
        /// Leave out items that match ones already in the library
        #[arg(long)]
        skip_duplicates: bool,
    },
    /// Serve Eagle's local HTTP API for a library on 127.0.0.1
    Serve {
        /// Path of the .library folder
        library: PathBuf,
        /// Port to listen on (Eagle itself uses 41595)
        #[arg(long, default_value_t = falcon_server::DEFAULT_PORT)]
        port: u16,
    },
}

impl Command {
    /// The library every command works on.
    fn library(&self) -> &std::path::Path {
        match self {
            Command::Import { library, .. }
            | Command::New { library }
            | Command::Info { library, .. }
            | Command::Folder { library, .. }
            | Command::Backup { library, .. }
            | Command::Export { library, .. }
            | Command::ImportPack { library, .. }
            | Command::Serve { library, .. } => library,
        }
    }
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
        Command::New { library } => {
            Library::create(&library)?;
            println!("{}", library.display());
        }
        Command::Folder { library, name, parent } => {
            let mut lib = Library::open(library)?;
            println!("{}", lib.create_folder(parent.as_deref(), &name)?);
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
        Command::Backup { library } => {
            let lib = Library::open(library)?;
            match lib.backup_metadata()? {
                Some(path) => println!("{}", path.display()),
                None => println!("no backup written (same size as the newest backup, or metadata.json is damaged)"),
            }
        }
        Command::Export { library, out, folder, items } => {
            let lib = Library::open(library)?;
            let source = match &folder {
                Some(id) => PackSource::Folder(id),
                None => PackSource::Items(&items),
            };
            let count = export_pack(&lib, source, &out)?;
            println!("{} ({count} items)", out.display());
        }
        Command::ImportPack { library, pack, folder, skip_duplicates } => {
            let mut lib = Library::open(library)?;
            for id in import_pack(&mut lib, &pack, &PackImportOptions { folder, skip_duplicates })? {
                println!("{id}");
            }
        }
        Command::Serve { library, port } => {
            let lib = Library::open(&library)?;
            println!("Starting the API for {} on http://127.0.0.1:{port}", library.display());
            let shared = std::sync::Arc::new(std::sync::Mutex::new(lib));
            if let Err(e) = falcon_server::serve(shared, port) {
                eprintln!("error: {e}");
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    falcon_library::sandbox::enforce(falcon_library::sandbox::default_root());
    let cli = Cli::parse();
    if let Err(e) = falcon_library::sandbox::guard(cli.command.library()) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
