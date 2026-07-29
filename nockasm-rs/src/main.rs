//! `nockasm`: the pure-Rust Nockasm compiler at the command line.
//!
//! Flag-compatible with `nasmc` (the Hoon-on-nockvm NockApp) so the
//! differential suite can drive both with identical invocations:
//!
//! ```text
//! nockasm program.nasm               # -> program.jam  (raw formula jam)
//! nockasm program.nasm -o out.jam
//! nockasm --text program.nasm       # canonical flat noun to stdout
//! nockasm --render program.nasm     # canonical .nasm formatting to stdout
//! nockasm --lift formula.jam        # -> formula.nasm (deterministic lift)
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: nockasm [--text | --render | --lift] [-o PATH] INPUT

Nockasm compiler: .nasm source in, Nock formula out (pure Rust).

modes (default: write the formula's jamfile next to the input):
  --text      print the canonical flat noun
  --render    reformat the source to canonical .nasm
  --lift      read INPUT as a jammed formula; emit canonical .nasm

options:
  -o, --output PATH   output path (default: <input>.jam, or <input>.nasm
                      for --lift; --text/--render default to stdout)
  --version           print the version
  -h, --help          print this help
";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Jam,
    Text,
    Render,
    Lift,
}

struct Cli {
    input: PathBuf,
    output: Option<PathBuf>,
    mode: Mode,
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut mode: Option<Mode> = None;
    let set_mode = |m: Mode, name: &str, mode: &mut Option<Mode>| {
        if mode.is_some() {
            return Err(format!("{name} conflicts with an earlier mode flag"));
        }
        *mode = Some(m);
        Ok(())
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let s = arg.to_string_lossy();
        match s.as_ref() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--version" => {
                println!("nockasm {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--text" => set_mode(Mode::Text, "--text", &mut mode)?,
            "--render" => set_mode(Mode::Render, "--render", &mut mode)?,
            "--lift" => set_mode(Mode::Lift, "--lift", &mut mode)?,
            "-o" | "--output" => {
                let path = args.next().ok_or("-o requires a path")?;
                output = Some(PathBuf::from(path));
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!("unknown flag {s}"));
            }
            _ => {
                if input.is_some() {
                    return Err("more than one input file".into());
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }
    let input = input.ok_or("an input file is required")?;
    Ok(Some(Cli {
        input,
        output,
        mode: mode.unwrap_or(Mode::Jam),
    }))
}

fn run(cli: Cli) -> Result<(), String> {
    let read = |p: &PathBuf| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let write = |p: &PathBuf, data: &[u8]| {
        std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display()))
    };
    let source = |bytes: Vec<u8>| {
        String::from_utf8(bytes)
            .map_err(|_| format!("{}: source is not valid UTF-8", cli.input.display()))
    };

    match cli.mode {
        Mode::Jam => {
            let src = source(read(&cli.input)?)?;
            let formula = nockasm::expand(&src).map_err(|e| e.to_string())?;
            let out = cli
                .output
                .clone()
                .unwrap_or_else(|| cli.input.with_extension("jam"));
            write(&out, &nockasm::jam(&formula))
        }
        Mode::Text => {
            let src = source(read(&cli.input)?)?;
            let formula = nockasm::expand(&src).map_err(|e| e.to_string())?;
            emit(&cli, format!("{formula}\n").as_bytes())
        }
        Mode::Render => {
            let src = source(read(&cli.input)?)?;
            let program = nockasm::parse(&src).map_err(|e| e.to_string())?;
            emit(&cli, program.render().as_bytes())
        }
        Mode::Lift => {
            let data = read(&cli.input)?;
            let text = nockasm::nasm_from_jam(&data).map_err(|e| e.to_string())?;
            let out = cli
                .output
                .clone()
                .unwrap_or_else(|| cli.input.with_extension("nasm"));
            write(&out, text.as_bytes())
        }
    }
}

/// Text modes print to stdout unless -o was given.
fn emit(cli: &Cli, data: &[u8]) -> Result<(), String> {
    match &cli.output {
        Some(p) => std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            use std::io::Write as _;
            std::io::stdout()
                .write_all(data)
                .map_err(|e| format!("stdout: {e}"))
        }
    }
}

fn main() -> ExitCode {
    match parse_args() {
        Err(e) => {
            eprintln!("nockasm: {e}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(cli)) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("nockasm: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
