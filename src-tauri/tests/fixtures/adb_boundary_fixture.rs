//! Test-only executable; never invokes ADB or touches a device.
use std::{env, fs, path::PathBuf, process};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("-s")
        && args.get(2).map(String::as_str) == Some("shell")
    {
        // Matches ADB's serialization at the remote-shell boundary.
        let source = args[3..].join(" ");
        if args[1] == "adapter-test" {
            let path: PathBuf = env::current_exe().unwrap().with_extension("capture.txt");
            fs::write(path, &source).unwrap();
        }
        println!("{source}");
        return;
    }
    eprintln!("protocol fault (isolated test fixture)");
    process::exit(23);
}
