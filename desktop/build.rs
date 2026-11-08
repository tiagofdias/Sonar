// Puts the Sonar icon inside the Windows .exe, for Explorer, shortcuts and pinned taskbar items.
fn main() {
    println!("cargo:rerun-if-changed=assets/sonar.ico");
    println!("cargo:rerun-if-changed=assets/sonar.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/sonar.rc", embed_resource::NONE);
    }
}
