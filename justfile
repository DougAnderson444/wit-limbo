build:
  cargo component build --target wasm32-unknown-unknown --release

test: build
  cargo test -- --nocapture
