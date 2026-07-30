//! nasmc-bench: time `%compile` pokes against a single booted kernel.
//!
//! nasmc proper boots its kernel per invocation (~450 ms, with enough
//! jitter to drown a corpus-sized expansion), so end-to-end wall time
//! cannot resolve how fast the Hoon expander actually runs on nockvm.
//! This tool boots once, then pokes N distinct comment-padded variants
//! of one source in sequence, timing each poke from submission to
//! kernel ack — and the ack fires only after the event's Nock
//! computation completes, so a poke time is kernel-side work: cause
//! dispatch, parse, expand, and (in %jam mode) the jam of the result.
//! Distinct variants keep `~+` parse memoization from collapsing
//! repeats. No file driver is attached, so nothing is written and the
//! kernel's exit-on-write-ack never fires; the process exits itself
//! after the last poke.
//!
//! Usage: nasmc-bench <input.nasm> [iterations] [mode]
//! Output: one TSV line per poke (`index <tab> ms`), then a summary.

use std::path::PathBuf;
use std::time::Instant;

use nockapp::driver::{make_driver, PokeResult};
use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::one_punch::OnePunchWire;
use nockapp::wire::Wire;
use nockapp::{AtomExt, NockApp};
use nockvm::noun::{Atom, D, T};
use nockvm_macros::tas;

static KERNEL_JAM: &[u8] = include_bytes!("../../bootstrap/nasmc.jam");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input: PathBuf = args
        .next()
        .ok_or("usage: nasmc-bench <input.nasm> [iterations] [mode]")?
        .into();
    let iterations: usize = args.next().map_or(Ok(50), |s| s.parse())?;
    let mode: String = args.next().unwrap_or_else(|| "jam".into());

    let src = std::fs::read_to_string(&input)?;
    let variants: Vec<String> = (0..iterations)
        .map(|k| format!("{src}\n; pad {k}\n"))
        .collect();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "error");
    }
    let mut boot_cli = boot::default_boot_cli(true);
    // State is throwaway: skip event persistence and fsync so a poke's
    // wall time is Nock computation, not the disk.
    boot_cli.ephemeral = true;
    boot_cli.disable_fsync = true;
    boot::init_default_tracing(&boot_cli);
    let data_root =
        std::env::temp_dir().join(format!("nasmc-bench-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_root);

    let boot_start = Instant::now();
    let mut nockapp: NockApp =
        boot::setup(KERNEL_JAM, boot_cli, &[], "nasmc", Some(data_root)).await?;
    eprintln!("boot: {:.1} ms", boot_start.elapsed().as_secs_f64() * 1e3);

    nockapp
        .add_io_driver(make_driver(move |handle| async move {
            let mut times_ms: Vec<f64> = Vec::with_capacity(variants.len());
            for (k, tex) in variants.iter().enumerate() {
                let mut slab: NounSlab = NounSlab::new();
                let mode_atom =
                    Atom::from_value(&mut slab, mode.as_bytes()).expect("mode atom").as_noun();
                let tex_atom =
                    Atom::from_value(&mut slab, tex.as_bytes()).expect("tex atom").as_noun();
                // The kernel echoes this path in its file effect; with no
                // file driver attached it is never written.
                let out_atom = Atom::from_value(&mut slab, "/dev/null")
                    .expect("out atom")
                    .as_noun();
                let poke = T(
                    &mut slab,
                    &[D(tas!(b"compile")), mode_atom, tex_atom, out_atom],
                );
                slab.set_root(poke);

                let start = Instant::now();
                let result = handle.poke(OnePunchWire::Poke.to_wire(), slab).await?;
                let ms = start.elapsed().as_secs_f64() * 1e3;
                assert!(matches!(result, PokeResult::Ack), "poke {k} nacked");
                println!("{k}\t{ms:.3}");
                times_ms.push(ms);
            }
            times_ms.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
            let median = times_ms[times_ms.len() / 2];
            let min = times_ms[0];
            eprintln!("median: {median:.3} ms  min: {min:.3} ms  n: {}", times_ms.len());
            std::process::exit(0);
        }))
        .await;
    nockapp.run().await?;
    Ok(())
}
