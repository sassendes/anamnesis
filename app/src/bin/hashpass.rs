use anamnesis::auth::hash_password;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let password = args.get(1).map(|s| s.as_str()).unwrap_or("anamnesis-admin");
    match hash_password(password) {
        Ok(h) => println!("{h}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
