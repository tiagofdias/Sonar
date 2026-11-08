//! Test-only bridge for synthetic Matrix attachment interoperability.
#[path = "../src/matrix_attachment.rs"] mod matrix_attachment;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Read;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new(); std::io::stdin().read_to_string(&mut input)?;
    let data: serde_json::Value = serde_json::from_str(&input)?;
    let bytes = STANDARD.decode(data["data"].as_str().ok_or("missing data")?)?;
    let output = match data["action"].as_str() {
        Some("encrypt") => { let (ciphertext, metadata) = matrix_attachment::encrypt(&bytes)?; serde_json::json!({"data":STANDARD.encode(ciphertext),"metadata":metadata}) },
        Some("decrypt") => { let plaintext = matrix_attachment::decrypt(&bytes, data["metadata"].as_str().ok_or("missing metadata")?)?; serde_json::json!({"data":STANDARD.encode(plaintext)}) },
        _ => return Err("invalid action".into()),
    };
    println!("{output}"); Ok(())
}
