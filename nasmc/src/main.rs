//! nasmc: the Nockasm compiler as a NockApp.
//!
//! Modeled on hoonc: the kernel (hoon/apps/nasmc/nasmc.hoon, compiled
//! by hoonc into bootstrap/nasmc.jam) wraps lib/nockasm; this host
//! reads the input file, pokes [%compile mode tex out], and lets the
//! file and exit drivers handle the write effect and shutdown.

use std::path::PathBuf;

use clap::Parser;
use nockapp::driver::Operation;
use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::{AtomExt, NockApp};
use nockvm::noun::{Atom, D, T};
use nockvm_macros::tas;

static KERNEL_JAM: &[u8] = include_bytes!("../bootstrap/nasmc.jam");

#[derive(Parser)]
#[command(
    name = "nasmc",
    about = "Nockasm compiler: .nasm source in, Nock formula out",
    version
)]
struct Cli {
    /// Input file: .nasm source (or a jammed formula, with --lift)
    input: PathBuf,

    /// Output path (default: <input>.jam for jam mode, stdout for
    /// the text modes, <input>.nasm for --lift)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Print the canonical flat noun instead of writing a jamfile
    #[arg(long, conflicts_with_all = ["render", "lift"])]
    text: bool,

    /// Reformat the source to canonical .nasm
    #[arg(long, conflicts_with_all = ["text", "lift"])]
    render: bool,

    /// Input is a jammed formula; emit canonical .nasm source
    #[arg(long, conflicts_with_all = ["text", "render"])]
    lift: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let mode: &[u8] = if cli.text {
        b"text"
    } else if cli.render {
        b"render"
    } else if cli.lift {
        b"lift"
    } else {
        b"jam"
    };

    let contents = std::fs::read(&cli.input)?;

    // text modes with no -o print to stdout; the effect still writes a
    // file (via a temp path) because the file driver is the transport
    let to_stdout = cli.output.is_none() && (cli.text || cli.render);
    let out_path = match &cli.output {
        Some(p) => p.clone(),
        None if mode == b"jam" => cli.input.with_extension("jam"),
        None if mode == b"lift" => cli.input.with_extension("nasm"),
        None => std::env::temp_dir()
            .join(format!("nasmc-out-{}", std::process::id())),
    };
    let out_abs = if out_path.is_absolute() {
        out_path
    } else {
        std::env::current_dir()?.join(out_path)
    };

    // a compiler should be quiet by default; RUST_LOG still overrides
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "error");
    }
    // new=true: a one-shot compiler wants no persistent kernel state
    let boot_cli = boot::default_boot_cli(true);
    boot::init_default_tracing(&boot_cli);

    // state is throwaway (new=true); keep the PMA out of the user's
    // cwd, unique per process, and clear any stale dir from PID reuse
    // (--new refuses a non-empty data directory)
    let data_root =
        std::env::temp_dir().join(format!("nasmc-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_root);
    let mut nockapp: NockApp =
        boot::setup(KERNEL_JAM, boot_cli, &[], "nasmc", Some(data_root)).await?;
    nockapp.add_io_driver(nockapp::file_driver()).await;
    nockapp.add_io_driver(nockapp::exit_driver()).await;

    let mut slab: NounSlab = NounSlab::new();
    let mode_atom = Atom::from_value(&mut slab, mode)?.as_noun();
    let tex = Atom::from_value(&mut slab, contents)?.as_noun();
    let out = Atom::from_value(
        &mut slab,
        out_abs.to_str().ok_or("output path is not valid UTF-8")?,
    )?
    .as_noun();
    let poke = T(&mut slab, &[D(tas!(b"compile")), mode_atom, tex, out]);
    slab.set_root(poke);

    // The compile poke yields effects (the file write), so it rides a
    // one-punch driver and run() processes them.
    nockapp
        .add_io_driver(nockapp::one_punch_driver(slab, Operation::Poke))
        .await;
    nockapp.run().await?;

    // The file driver writes atoms padded to 64-bit words. No nasmc
    // output legitimately ends in NUL (text never contains it; a jam's
    // value is unchanged by trailing zeros), so right-trim the file.
    let data = std::fs::read(&out_abs)?;
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    if to_stdout {
        use std::io::Write as _;
        std::io::stdout().write_all(&data[..end])?;
        let _ = std::fs::remove_file(&out_abs);
    } else if end < data.len() {
        std::fs::write(&out_abs, &data[..end])?;
    }
    Ok(())
}
