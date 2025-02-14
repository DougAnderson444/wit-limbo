//! This test use [wasm_component_layer] which gives us flexibility to define the host runtime,
//! but it is slower to run because the bytes have to be read into memory, as opposed
//! to being read from disk like when using wasmtime.
//!
//! Use this model when you need runtime agnostic code, or when you need to define your own
//! host runtime.  Otherwise on native targets, use the wasmtime runtime layer as it's faster.
//!
use std::path::{Path, PathBuf};

use wasm_component_layer::*;

// Note: wasmi is way faster than wasmtime when using the layer
//use wasmtime_runtime_layer as runtime_layer;
use wasmi_runtime_layer as runtime_layer;

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

#[test]
fn test_wasm_component_layer_instance() {
    // log with timstamp
    eprintln!("{} [TestLog] test_instantiate_instance", chrono::Utc::now());

    // get the target/wasm32-wasi/debug/CARGO_PKG_NAME.wasm file
    let pkg_name = std::env::var("CARGO_PKG_NAME").unwrap().replace('-', "_");
    let workspace = workspace_dir();
    let wasm_path = format!("target/wasm32-unknown-unknown/release/{}.wasm", pkg_name);
    let wasm_path = workspace.join(wasm_path);

    //let bytes: &[u8] =
    //    include_bytes!("../../../target/wasm32-unknown-unknown/release/wit_limbo.wasm");

    let bytes = std::fs::read(wasm_path).unwrap();

    let data = ();

    // Create a new engine for instantiating a component.
    let engine = Engine::new(runtime_layer::Engine::default());

    // Create a store for managing WASM data and any custom user-defined state.
    let mut store = Store::new(&engine, data);

    eprintln!(
        "{} [TestLog] Created store, loading bytes.",
        chrono::Utc::now()
    );
    // Parse the component bytes and load its imports and exports.
    let component = Component::new(&engine, &bytes).unwrap();

    eprintln!("{} [TestLog] Loaded bytes", chrono::Utc::now());

    // Create a linker that will be used to resolve the component's imports, if any.
    let mut linker = Linker::default();

    let host_interface = linker
        .define_instance("component:wit-limbo/host".try_into().unwrap())
        .unwrap();

    host_interface
        .define_func(
            "log",
            Func::new(
                &mut store,
                FuncType::new([ValueType::String], []),
                move |_store, params, _results| {
                    if let Value::String(s) = &params[0] {
                        eprintln!("{}", s);
                    }
                    Ok(())
                },
            ),
        )
        .unwrap();

    // func "random-byte" is defined in the host interface
    host_interface
        .define_func(
            "random-byte",
            Func::new(
                &mut store,
                FuncType::new([], [ValueType::U8]),
                move |_store, _params, results| {
                    let random = rand::random::<u8>();
                    results[0] = Value::U8(random);
                    Ok(())
                },
            ),
        )
        .unwrap();

    // Instantiate the component with the linker and store.
    let instance = linker.instantiate(&mut store, &component).unwrap();

    // Get the interface that the interface exports.
    let exports = instance.exports();

    // Get the interface that the interface exports.
    let interface = exports
        .instance(&"component:wit-limbo/limbo".try_into().unwrap())
        .unwrap();

    // Call the resource constructor for 'bar' using a direct function call
    let resource_constructor = interface.func("[constructor]database").unwrap();

    // We need to provide a mutable reference to store the results.
    // This can be any Value type, as it will get overwritten by the result.
    // It is a Value::Bool here but will be overwritten by a Value::Own(ResourceOwn)
    // after we call the constructor.
    let mut results = vec![Value::Bool(false)];
    let arguments = &[Value::String(":memory:".to_string().into())];

    eprintln!(
        "{} [TestLog] Calling resource constructor",
        chrono::Utc::now()
    );

    // Construct the resource with the argument `42`
    resource_constructor
        .call(&mut store, arguments, &mut results)
        .unwrap();

    let database_resource = match results[0] {
        Value::Own(ref resource) => resource.clone(),
        _ => panic!("Unexpected result type"),
    };

    let borrowed_db = database_resource.borrow(store.as_context_mut()).unwrap();

    // argument are: 1) The borrowed resource, and 2) the SQL statement
    // Let's make the sqlite statement to be Create Table users
    let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".to_string();
    let exec_arguments = vec![
        Value::Borrow(borrowed_db.clone()),
        Value::String(sql.into()),
    ];

    eprintln!("{} [TestLog] Calling database.exec", chrono::Utc::now());

    // method database exec
    let method_database_exec = interface.func("[method]database.exec").unwrap();

    method_database_exec
        .call(&mut store, &exec_arguments, &mut [])
        .unwrap();

    // Insert user into the database
    let exec_arguments = vec![
        Value::Borrow(borrowed_db.clone()),
        Value::String("INSERT INTO users (name) VALUES ('Alice');".into()),
    ];

    eprintln!("{} [TestLog] Calling database.exec", chrono::Utc::now());

    // Call the method, mutate the results
    method_database_exec
        .call(&mut store, &exec_arguments, &mut [])
        .unwrap();

    // Get the `value` method of the `bar` resource
    let method_prepare = interface.func("[method]database.prepare").unwrap();

    let sql = "SELECT * FROM users;".to_string();
    let prepare_arguments = vec![
        Value::Borrow(borrowed_db.clone()),
        Value::String(sql.into()),
    ];
    let mut results = [Value::Bool(false)];

    eprintln!("{} [TestLog] Calling database.prepare", chrono::Utc::now());

    // Call the method, mutate the results
    method_prepare
        .call(&mut store, &prepare_arguments, &mut results)
        .unwrap();

    let statement_resource = match results[0] {
        Value::Own(ref resource) => resource.clone(),
        _ => panic!("Unexpected result type"),
    };

    // Now use the statement resource to call [method]statement.all to get all results
    let borrowed_stmt = statement_resource.borrow(store.as_context_mut()).unwrap();

    let method_all = interface.func("[method]statement.all").unwrap();

    let mut results = [Value::Bool(false)];

    eprintln!("{} [TestLog] Calling statement.all", chrono::Utc::now());

    // Call the method, mutate the results
    method_all
        .call(
            &mut store,
            &[Value::Borrow(borrowed_stmt.clone())],
            &mut results,
        )
        .unwrap();

    eprintln!(
        "{} [TestLog] Finished calling statement.all",
        chrono::Utc::now()
    );

    let list = match results[0] {
        Value::List(ref list) => list.clone(),
        _ => panic!("Expected List, found Unexpected result type"),
    };

    println!("[ResultLog]");
    println!(" └ database.prepare() =");
    // enumerate each row, then each column
    for (i, row) in list.iter().enumerate() {
        println!("    └ Row {}", i);
        let row = match row {
            Value::List(ref list) => list,
            _ => panic!("Expected List, found Unexpected result type"),
        };

        print!("       └ ");
        for (j, column) in row.iter().enumerate() {
            print!(" ");
            match column {
                Value::Variant(ref variant) => {
                    let variant = variant.clone();
                    let value = variant.value();
                    match value {
                        Some(Value::S64(v)) => print!("       └ Column {}: {:?}, ", j, v),
                        Some(Value::String(v)) => print!("       └ Column {}: {:?}, ", j, v),
                        _ => print!(": {:?}", value),
                    }
                }
                _ => panic!("Expected Variant, found Unexpected result type"),
            }
        }
        println!("\n\n");
    }

    let record_value_ty = VariantType::new(
        None,
        vec![
            VariantCase::new("null", None),
            VariantCase::new("integer", Some(ValueType::S64)),
            VariantCase::new("float", Some(ValueType::F64)),
            VariantCase::new("text", Some(ValueType::String)),
            VariantCase::new("blob", Some(ValueType::List(ListType::new(ValueType::U8)))),
        ],
    )
    .unwrap();

    let list_type = ListType::new(ValueType::List(ListType::new(ValueType::Variant(
        record_value_ty.clone(),
    ))));

    let expected_list = List::new(
        list_type.clone(),
        vec![Value::List(
            List::new(
                ListType::new(ValueType::Variant(record_value_ty.clone())),
                vec![
                    Value::Variant(
                        Variant::new(record_value_ty.clone(), 1, Some(Value::S64(1))).unwrap(),
                    ),
                    Value::Variant(
                        Variant::new(
                            record_value_ty.clone(),
                            3,
                            Some(Value::String("Alice".to_string().into())),
                        )
                        .unwrap(),
                    ),
                ],
            )
            .unwrap(),
        )],
    )
    .unwrap();

    assert_eq!(list, expected_list);
}
