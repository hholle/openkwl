//! Prints the default `CONFIG.INI`: `cargo run -p openkwl-config --example default_config`.

fn main() {
    let mut out = String::new();
    openkwl_config::ini::write_config(&openkwl_config::Config::default(), &mut out).expect("formatting into a String");
    print!("{out}");
}
