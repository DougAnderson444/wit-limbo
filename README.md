# Limbo running in a WebAssembly Component

We all know about wasm-bindgen, but what have you heard about wasm components?

WebAssembly is evolving from being more than just a Module in JavaScript via `wasm-bindgen`. The next iteration of the wasm model is the component model, which involves identifying a Wasm Interface Type (WIT) system for interacting with the WebAssembly code.

This is an experiment on an experiment with [Limbo](https://github.com/tursodatabase/limbo) to see if we could compile and run SQLite database inside a wasm-component.

Turns out, we can!

To run the build and test:

```sh
# build:
cargo component build --target wasm32-unknown-unknown --release

# test: build
cargo test -- --nocapture
```

If you have [just.systems](https://just.systems) installed, you can run the tests with the just commands at [./justfile](./justfile).

## Tests

There are 2 tests:

1. [`wasm_component_layer`](https://github.com/DouglasDwyer/wasm_component_layer): Which is a runtime agnostic layer for running wasm components anywhere. It's very flexible and allows you to have isomorphic code on native and browser. 
2. [`wasmtime`](https://github.com/bytecodealliance/wasmtime): Specific runtime, but loads faster than the layer because it loads the wasm bytes directly from disk, whereas the layer copies to memory, resulting in a slower load. The downside of wasmtime is you need to use their syntax which means you'll have to re-write your host code for running on any other host.

See the tests as example SQLite usage:

- [`wasm_component_layer`](./tests/test_wasm_component_layer.rs) 
- [`wasmtime`](./tests/test_wasmtime.rs)

## WIT Composable Components: SQLite runtime extensions?

The next idea would be to compose wasm components together in order to create SQLite runtime extensions. This gives us the security of the wasm sandbox model, yet the flexibility of runtime loading. This is an unimplemented idea.
