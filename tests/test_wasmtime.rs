//! Test directly with wasmtime. This is faster than using [wasm_component_layer], however you
//! lose flexibility in runtime host and code reuse. You need to have different code for native and
//! javascript targets, but the wasmtime is faster than [wasm_component_layer]. It's a tradeoff.
mod bindgen {
    wasmtime::component::bindgen!();
}

use std::{
    env,
    path::{Path, PathBuf},
};
use thiserror::Error;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

use bindgen::{
    component::wit_limbo::host,
    exports::component::wit_limbo::limbo::{
        Database, Guest, GuestDatabase, RecordValue, Statement,
    },
};

struct MyCtx {
    table: ResourceTable,
    ctx: WasiCtx,
}

impl WasiView for MyCtx {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

impl host::Host for MyCtx {
    fn random_byte(&mut self) -> u8 {
        rand::random::<u8>()
    }

    fn log(&mut self, message: String) {
        eprintln!("{}", message);
    }
}

#[derive(Error, Debug)]
pub enum TestError {
    /// From String
    #[error("Error message {0}")]
    Stringified(String),

    /// From Wasmtime
    #[error("Wasmtime: {0}")]
    Wasmtime(#[from] wasmtime::Error),

    /// From VarError
    #[error("VarError: {0}")]
    VarError(#[from] std::env::VarError),

    /// From io
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
}

impl From<String> for TestError {
    fn from(s: String) -> Self {
        TestError::Stringified(s)
    }
}

/// Utility function to get the workspace dir
pub fn workspace_dir() -> PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    let cargo_path = Path::new(std::str::from_utf8(&output).unwrap().trim());
    cargo_path.parent().unwrap().to_path_buf()
}

#[cfg(test)]
mod aggregate_peerpiper_tests {

    use crate::bindgen;

    use super::*;

    #[test]
    fn test_wasmtime_load() -> wasmtime::Result<(), TestError> {
        eprintln!("{} [TestLog] test_start", chrono::Utc::now());

        // get the target/wasm32-wasi/debug/CARGO_PKG_NAME.wasm file
        let pkg_name = std::env::var("CARGO_PKG_NAME")?.replace('-', "_");
        let workspace = workspace_dir();
        let wasm_path = format!("target/wasm32-unknown-unknown/release/{}.wasm", pkg_name);
        let wasm_path = workspace.join(wasm_path);

        let mut config = Config::new();
        config.cache_config_load_default()?;
        config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        config.wasm_component_model(true);

        let engine = Engine::new(&config)?;

        eprintln!(
            "{} [TestLog] Created store, loading bytes.",
            chrono::Utc::now()
        );

        let component = Component::from_file(&engine, &wasm_path)?;

        eprintln!("{} [TestLog] Loaded bytes", chrono::Utc::now());

        let mut linker = Linker::new(&engine);
        // link imports like get_seed to our instantiation
        bindgen::Example::add_to_linker(&mut linker, |state: &mut MyCtx| state)?;
        // link the WASI imports to our instantiation
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        let table = ResourceTable::new();
        let wasi: WasiCtx = WasiCtxBuilder::new().inherit_stdout().args(&[""]).build();
        let state = MyCtx { table, ctx: wasi };
        let mut store = Store::new(&engine, state);

        let bindings = bindgen::Example::instantiate(&mut store, &component, &linker)?;

        eprintln!(
            "{} [TestLog] Calling resource constructor",
            chrono::Utc::now()
        );

        // Use bindings
        let resource_constructor = bindings
            .component_wit_limbo_limbo()
            .database()
            .call_constructor(&mut store, ":memory:")?;

        let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".to_string();

        // call database method exec with sql
        bindings.component_wit_limbo_limbo().database().call_exec(
            &mut store,
            resource_constructor,
            &sql,
        )?;

        let sql = "INSERT INTO users (name) VALUES ('Alice');".to_string();

        // call database method exec with sql
        bindings.component_wit_limbo_limbo().database().call_exec(
            &mut store,
            resource_constructor,
            &sql,
        )?;

        let sql = "SELECT * FROM users;".to_string();
        let statement = bindings
            .component_wit_limbo_limbo()
            .database()
            .call_prepare(&mut store, resource_constructor, &sql)?;

        // call all using the statement result
        let rows = bindings
            .component_wit_limbo_limbo()
            .statement()
            .call_all(&mut store, statement)?;

        println!("[ResultLog]");
        println!(" └ database.prepare() =");

        for (i, row) in rows.iter().enumerate() {
            println!("    └ Row {}", i);
            for (j, col) in row.iter().enumerate() {
                match col {
                    RecordValue::Integer(i) => {
                        print!("       └ Column {}: {:?}", j, i);
                    }
                    RecordValue::Text(s) => {
                        print!("       └ Column {}: {:?}", j, s);
                    }
                    _ => print!("       └ Column: {:?}", col),
                }
            }
        }

        Ok(())
    }
}
