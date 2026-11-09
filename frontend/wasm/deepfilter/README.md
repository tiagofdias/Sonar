# Browser noise suppression engine

DeepFilterNet3 (MIT or Apache-2.0, through the `deep-filter-rt` crate) compiled to WebAssembly, so the web app runs the same engine as the desktop app. The built file is committed at `frontend/public/deepfilter/engine.bin`, so deployments do not need Rust.

To rebuild, from this folder:

    rustup target add wasm32-unknown-unknown
    cargo build --release
    python -c "import gzip,shutil; shutil.copyfileobj(open('target/wasm32-unknown-unknown/release/sonar_deepfilter.wasm','rb'), gzip.open('../../public/deepfilter/engine.bin','wb',9))"
