//! The CLI without the app around it, for development: `cargo run -p weave-cli --example weave -- view a.docx`.
fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    std::process::exit(weave_cli::main(&args));
}
