//! Minimal private wrapper around the Windows-serviced SQLite runtime.
use crate::{Error, Result};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    marker::PhantomData,
    path::Path,
    ptr::{null, null_mut},
};
type Raw = *mut c_void;
#[link(name = "winsqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut Raw,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: Raw) -> c_int;
    fn sqlite3_busy_timeout(db: Raw, milliseconds: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        db: Raw,
        sql: *const c_char,
        size: c_int,
        stmt: *mut Raw,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: Raw) -> c_int;
    fn sqlite3_step(stmt: Raw) -> c_int;
    fn sqlite3_bind_text(
        stmt: Raw,
        index: c_int,
        value: *const c_char,
        len: c_int,
        destructor: Option<unsafe extern "C" fn(Raw)>,
    ) -> c_int;
    fn sqlite3_bind_blob(
        stmt: Raw,
        index: c_int,
        value: *const c_void,
        len: c_int,
        destructor: Option<unsafe extern "C" fn(Raw)>,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: Raw, index: c_int, value: i64) -> c_int;
    fn sqlite3_column_text(stmt: Raw, index: c_int) -> *const u8;
    fn sqlite3_column_blob(stmt: Raw, index: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: Raw, index: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: Raw, index: c_int) -> i64;
    fn sqlite3_changes(db: Raw) -> c_int;
}
fn check(code: c_int) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Database(code))
    }
}
pub struct Db(Raw);
impl Drop for Db {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close_v2(self.0);
        }
    }
}
impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::Invalid);
        }
        let name =
            CString::new(path.to_str().ok_or(Error::Invalid)?).map_err(|_| Error::Invalid)?;
        let mut db = null_mut();
        let code = unsafe { sqlite3_open_v2(name.as_ptr(), &mut db, 2 | 4 | 0x10000, null()) };
        if db.is_null() {
            return Err(Error::Database(code));
        }
        let result = Self(db);
        check(code)?;
        unsafe {
            check(sqlite3_busy_timeout(db, 2000))?;
        }
        Ok(result)
    }
    pub fn prepare(&self, sql: &str) -> Result<Stmt<'_>> {
        let sql = CString::new(sql).map_err(|_| Error::Invalid)?;
        let mut raw = null_mut();
        unsafe {
            check(sqlite3_prepare_v2(
                self.0,
                sql.as_ptr(),
                -1,
                &mut raw,
                null_mut(),
            ))?;
        }
        if raw.is_null() {
            return Err(Error::Invalid);
        }
        Ok(Stmt {
            raw,
            texts: Vec::new(),
            blobs: Vec::new(),
            _db: PhantomData,
        })
    }
    pub fn exec(&self, sql: &str) -> Result<()> {
        let mut statement = self.prepare(sql)?;
        while statement.step()? {}
        Ok(())
    }
    pub fn changes(&self) -> i32 {
        unsafe { sqlite3_changes(self.0) }
    }
    pub fn scalar(&self, sql: &str) -> Result<String> {
        let mut statement = self.prepare(sql)?;
        if !statement.step()? {
            return Err(Error::Invalid);
        }
        statement.text(0)
    }
}
pub struct Stmt<'a> {
    raw: Raw,
    texts: Vec<CString>,
    blobs: Vec<Vec<u8>>,
    _db: PhantomData<&'a Db>,
}
impl Drop for Stmt<'_> {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.raw);
        }
    }
}
impl Stmt<'_> {
    pub fn bind_text(&mut self, index: i32, value: &str) -> Result<()> {
        if value.len() > 8192 {
            return Err(Error::Invalid);
        }
        let text = CString::new(value).map_err(|_| Error::Invalid)?;
        self.texts.push(text);
        let text = self.texts.last().unwrap();
        unsafe {
            check(sqlite3_bind_text(
                self.raw,
                index,
                text.as_ptr(),
                value.len() as i32,
                None,
            ))
        }
    }
    pub fn bind_blob(&mut self, index: i32, value: &[u8]) -> Result<()> {
        if value.is_empty() || value.len() > 8192 {
            return Err(Error::Invalid);
        }
        self.blobs.push(value.to_vec());
        let value = self.blobs.last().unwrap();
        unsafe {
            check(sqlite3_bind_blob(
                self.raw,
                index,
                value.as_ptr().cast(),
                value.len() as i32,
                None,
            ))
        }
    }
    pub fn bind_integer(&mut self, index: i32, value: i64) -> Result<()> {
        unsafe { check(sqlite3_bind_int64(self.raw, index, value)) }
    }
    pub fn step(&mut self) -> Result<bool> {
        match unsafe { sqlite3_step(self.raw) } {
            100 => Ok(true),
            101 => Ok(false),
            code => Err(Error::Database(code)),
        }
    }
    pub fn text(&self, index: i32) -> Result<String> {
        unsafe {
            let pointer = sqlite3_column_text(self.raw, index);
            let length = sqlite3_column_bytes(self.raw, index);
            if pointer.is_null() || !(0..=8192).contains(&length) {
                return Err(Error::Invalid);
            }
            String::from_utf8(std::slice::from_raw_parts(pointer, length as usize).to_vec())
                .map_err(|_| Error::Invalid)
        }
    }
    pub fn blob(&self, index: i32) -> Result<Vec<u8>> {
        unsafe {
            let pointer = sqlite3_column_blob(self.raw, index);
            let length = sqlite3_column_bytes(self.raw, index);
            if pointer.is_null() || !(1..=8192).contains(&length) {
                return Err(Error::Invalid);
            }
            Ok(std::slice::from_raw_parts(pointer.cast::<u8>(), length as usize).to_vec())
        }
    }
    pub fn integer(&self, index: i32) -> i64 {
        unsafe { sqlite3_column_int64(self.raw, index) }
    }
}
