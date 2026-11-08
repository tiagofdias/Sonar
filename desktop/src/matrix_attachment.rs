//! Bounded Matrix attachment encryption. Metadata contains the secret key and
//! must only be included in an authenticated encrypted message.
#![allow(dead_code)]
use matrix_sdk_crypto::{AttachmentDecryptor, AttachmentEncryptor, MediaEncryptionInfo};
use std::io::{Cursor, Read};
use zeroize::Zeroize;

pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub fn encrypt(data: &[u8]) -> Result<(Vec<u8>, String), String> {
    if data.is_empty() || data.len() > MAX_BYTES { return Err("Tamanho do anexo invalido".into()); }
    let mut input = Cursor::new(data);
    let mut encryptor = AttachmentEncryptor::new(&mut input);
    let mut ciphertext = Vec::with_capacity(data.len());
    encryptor.read_to_end(&mut ciphertext).map_err(|_| "Nao foi possivel cifrar o anexo")?;
    let metadata = serde_json::to_string(&encryptor.finish()).map_err(|_| "Metadados do anexo invalidos")?;
    Ok((ciphertext, metadata))
}
pub fn decrypt(ciphertext: &[u8], metadata: &str) -> Result<Vec<u8>, String> {
    if ciphertext.is_empty() || ciphertext.len() > MAX_BYTES || metadata.len() > 4096 { return Err("Anexo invalido".into()); }
    let info: MediaEncryptionInfo = serde_json::from_str(metadata).map_err(|_| "Metadados do anexo invalidos")?;
    let mut input = Cursor::new(ciphertext);
    let mut decryptor = AttachmentDecryptor::new(&mut input, info).map_err(|_| "Metadados do anexo invalidos")?;
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    if decryptor.read_to_end(&mut plaintext).is_err() { plaintext.zeroize(); return Err("A integridade do anexo falhou".into()); }
    Ok(plaintext)
}
