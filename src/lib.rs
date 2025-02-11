#![allow(clippy::arc_with_non_send_sync)]

#[allow(warnings)]
mod bindings;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use bindings::exports::component::wit_limbo;
use bindings::{
    component::wit_limbo::host::random_byte,
    exports::component::wit_limbo::limbo::{
        Guest, GuestDatabase, GuestStatement, RecordValue, Statement as WitStatement,
    },
};

use limbo_core::{
    maybe_init_database_file, BufferPool, Database, MemoryIO, Pager, Result, WalFile, WalFileShared,
};

/// Custom function to use the import for random byte generation.
///
/// We do this is because "js" feature is incompatible with the component model
/// if you ever got the __wbindgen_placeholder__ error when trying to use the `js` feature
/// of getrandom,
fn imported_random(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    // iterate over the length of the destination buffer and fill it with random bytes
    (0..dest.len()).for_each(|i| {
        dest[i] = random_byte();
    });

    Ok(())
}

getrandom::register_custom_getrandom!(imported_random);

struct Component {
    inner: Arc<Database>,
    conn: Rc<limbo_core::Connection>,
}

impl Guest for Component {
    type Database = Component;

    type Statement = InnerStatement;
}

impl GuestDatabase for Component {
    fn new(path: String) -> Self {
        match path.as_str() {
            ":memory:" => {
                let io: Arc<dyn limbo_core::IO> = Arc::new(MemoryIO::new().unwrap());

                let file = io
                    .open_file(&path, limbo_core::OpenFlags::Create, false)
                    .unwrap();

                maybe_init_database_file(&file, &io).unwrap();
                let page_io = Rc::new(DatabaseStorage::new(file));
                let db_header = Pager::begin_open(page_io.clone()).unwrap();

                // ensure db header is there
                io.run_once().unwrap();

                let page_size = db_header.borrow().page_size;

                let wal_path = format!("{}-wal", path);
                let wal_shared =
                    WalFileShared::open_shared(&io, wal_path.as_str(), page_size).unwrap();
                let buffer_pool = Rc::new(BufferPool::new(page_size as usize));
                let wal = Rc::new(RefCell::new(WalFile::new(
                    io.clone(),
                    db_header.borrow().page_size as usize,
                    wal_shared.clone(),
                    buffer_pool.clone(),
                )));

                let db =
                    limbo_core::Database::open(io, page_io, wal, wal_shared, buffer_pool).unwrap();

                let conn = db.connect();
                Self { inner: db, conn }
            }
            _ => todo!(),
        }
    }

    fn exec(&self, sql: String) {
        self.conn.execute(sql).unwrap();
    }

    fn prepare(&self, sql: String) -> WitStatement {
        let stmt = self.conn.prepare(sql).unwrap();
        let inner_stmt = InnerStatement::new(stmt, false);
        WitStatement::new(inner_stmt)
    }
}

struct InnerStatement {
    inner: RefCell<limbo_core::Statement>,
    raw: bool,
}

impl InnerStatement {
    fn new(stmt: limbo_core::Statement, raw: bool) -> Self {
        Self {
            inner: RefCell::new(stmt),
            raw,
        }
    }
}

impl GuestStatement for InnerStatement {
    fn all(&self) -> Vec<Vec<RecordValue>> {
        let mut ret = vec![];
        loop {
            let mut stmt = self.inner.borrow_mut();
            match stmt.step() {
                Ok(limbo_core::StepResult::Row) => {
                    let row = stmt.row().unwrap();
                    let mut row_array = vec![];
                    for value in row.get_values() {
                        let value = value.to_value();
                        //let value = to_js_value(value);
                        row_array.push(value.into());
                    }
                    ret.push(row_array);
                }
                Ok(limbo_core::StepResult::IO) => {}
                Ok(limbo_core::StepResult::Interrupt) => break,
                Ok(limbo_core::StepResult::Done) => break,
                Ok(limbo_core::StepResult::Busy) => break,
                Err(e) => panic!("Error: {:?}", e),
            }
        }
        ret
    }
}

impl From<limbo_core::Value<'_>> for RecordValue {
    fn from(value: limbo_core::Value) -> Self {
        match value {
            limbo_core::Value::Null => RecordValue::Null,
            limbo_core::Value::Integer(i) => RecordValue::Integer(i),
            limbo_core::Value::Float(f) => RecordValue::Float(f),
            limbo_core::Value::Text(s) => RecordValue::Text(s.to_string()),
            limbo_core::Value::Blob(b) => RecordValue::Blob(b.to_vec()),
        }
    }
}

bindings::export!(Component with_types_in bindings);

pub struct DatabaseStorage {
    file: Rc<dyn limbo_core::File>,
}

impl DatabaseStorage {
    pub fn new(file: Rc<dyn limbo_core::File>) -> Self {
        Self { file }
    }
}

impl limbo_core::DatabaseStorage for DatabaseStorage {
    fn read_page(&self, page_idx: usize, c: limbo_core::Completion) -> Result<()> {
        let r = match c {
            limbo_core::Completion::Read(ref r) => r,
            _ => unreachable!(),
        };
        let size = r.buf().len();
        assert!(page_idx > 0);
        if !(512..=65536).contains(&size) || size & (size - 1) != 0 {
            return Err(limbo_core::LimboError::NotADB);
        }
        let pos = (page_idx - 1) * size;
        self.file.pread(pos, c)?;
        Ok(())
    }

    fn write_page(
        &self,
        page_idx: usize,
        buffer: Rc<std::cell::RefCell<limbo_core::Buffer>>,
        c: limbo_core::Completion,
    ) -> Result<()> {
        let size = buffer.borrow().len();
        let pos = (page_idx - 1) * size;
        self.file.pwrite(pos, buffer, c)?;
        Ok(())
    }

    fn sync(&self, _c: limbo_core::Completion) -> Result<()> {
        todo!()
    }
}
