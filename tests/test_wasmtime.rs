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

use bindgen::{component::wit_limbo::host, exports::component::wit_limbo::limbo::RecordValue};

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

    use std::fmt::{Display, Formatter};

    use super::*;

    use crate::bindgen;

    #[derive(Debug)]
    pub struct ColumnInfo {
        ///  Column ID
        pub cid: i64,
        /// Column Name
        pub name: String,
        /// Column Type
        pub ty: String,
        /// Column Not Null
        pub is_nullable: bool,
        /// Column Default Value
        pub default_value: Option<String>,
        /// Column Primary Key
        pub is_pk: bool,
    }

    impl Display for ColumnInfo {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            // make the display format dependent on whether the column is nullable or not
            // if it IS something, display it. If it isn't, hide it.
            // If it's nullable simply wirte NULLABLE, same for Primary Key.
            // If there's a Defualt Value, display it. If no default, hide it.
            match (self.is_nullable, self.default_value.is_some(), self.is_pk) {
                (true, true, true) => write!(
                    f,
                    "Column: {} {} NOT NULL DEFAULT {} PRIMARY KEY",
                    self.name,
                    self.ty,
                    self.default_value.as_ref().unwrap()
                ),
                (true, true, false) => write!(
                    f,
                    "Column: {} {} NOT NULL DEFAULT {}",
                    self.name,
                    self.ty,
                    self.default_value.as_ref().unwrap()
                ),
                (true, false, true) => {
                    write!(f, "Column: {} {} NOT NULL PRIMARY KEY", self.name, self.ty)
                }
                (true, false, false) => write!(f, "Column: {} {} NOT NULL", self.name, self.ty),
                (false, true, true) => write!(
                    f,
                    "Column: {} {} DEFAULT {} PRIMARY KEY",
                    self.name,
                    self.ty,
                    self.default_value.as_ref().unwrap()
                ),
                (false, true, false) => write!(
                    f,
                    "Column: {} {} DEFAULT {}",
                    self.name,
                    self.ty,
                    self.default_value.as_ref().unwrap()
                ),
                (false, false, true) => write!(f, "Column: {} {} PRIMARY KEY", self.name, self.ty),
                (false, false, false) => write!(f, "Column: {} {}", self.name, self.ty),
            }
        }
    }

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

        let sql_table_metadata = "PRAGMA table_info(users)".to_string();

        let statement = bindings
            .component_wit_limbo_limbo()
            .database()
            .call_prepare(&mut store, resource_constructor, &sql_table_metadata)?;

        let mut headers = bindings
            .component_wit_limbo_limbo()
            .statement()
            .call_all(&mut store, statement)?;

        eprintln!("\n\n{:?}\n\n", headers);

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
        println!(" └ database");

        // Show the Headers first. This is the column names.
        print!("    └ Headers: ");
        for header in headers.iter_mut() {
            // Column ID, Name, Type, NotNull, Default Value, Primary Key
            if let [RecordValue::Integer(cid), RecordValue::Text(name), RecordValue::Text(ty), RecordValue::Integer(notnull), RecordValue::Null, RecordValue::Integer(pk)] =
                &header[..]
            {
                let column_info = ColumnInfo {
                    cid: *cid,
                    name: name.to_string(),
                    ty: ty.to_string(),
                    is_nullable: notnull == &0,
                    default_value: None,
                    is_pk: pk == &1,
                };
                print!("{}", column_info);
            } else {
                print!("Not printable");
            }

            print!(" | ");
        }

        println!();

        for (i, row) in rows.iter().enumerate() {
            println!("    └ Row {}", i);
            for (j, col) in row.iter().enumerate() {
                match col {
                    RecordValue::Integer(i) => {
                        print!("       └ Row {}: {:?}", j, i);
                    }
                    RecordValue::Text(s) => {
                        print!("       └ Row {}: {:?}", j, s);
                    }
                    _ => print!("       └ Row {:?}", col),
                }
            }
        }
        println!("\n");

        Ok(())
    }
}
