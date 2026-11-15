//! Audit committed shuffles and public economic transitions from a bounded JSON export.
use std::io::Read;
fn main() {
    let result = (|| {
        let mut input = String::new();
        std::io::stdin()
            .take(4 * 1024 * 1024 + 1)
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 4 * 1024 * 1024 {
            return Err("replay exceeds 4 MiB".into());
        }
        pokerdraft_engine::replay::verify(serde_json::from_str(&input).map_err(|e| e.to_string())?)
    })();
    match result {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("replay verification failed: {e}");
            std::process::exit(1);
        }
    }
}
