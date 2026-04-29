use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow,
};
use sqlx::Row as SqlxRow;

pub type Result<T> = std::result::Result<T, Error>;
type SharedTx = Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Sqlite>>>>;

#[derive(Debug, Clone)]
pub enum Error {
    QueryReturnedNoRows,
    InvalidColumnName(String),
    Message(String),
}

impl Error {
    fn sqlx(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::QueryReturnedNoRows,
            other => Self::Message(other.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueryReturnedNoRows => write!(f, "query returned no rows"),
            Self::InvalidColumnName(name) => write!(f, "invalid column name: {name}"),
            Self::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct SqliteConnectionManager {
    path: PathBuf,
}

impl SqliteConnectionManager {
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

#[derive(Debug)]
pub struct Pool<T = SqliteConnectionManager> {
    inner: SqlitePool,
    _marker: PhantomData<T>,
}

impl<T> Clone for Pool<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl Pool<SqliteConnectionManager> {
    pub fn builder() -> PoolBuilder {
        PoolBuilder { max_size: 1 }
    }

    pub fn get(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        Ok(Connection {
            pool: self.inner.clone(),
            tx: None,
            _marker: PhantomData,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PoolBuilder {
    max_size: u32,
}

impl PoolBuilder {
    pub fn max_size(mut self, max_size: u32) -> Self {
        self.max_size = max_size.max(1);
        self
    }

    pub fn build(self, manager: SqliteConnectionManager) -> Result<Pool<SqliteConnectionManager>> {
        let url = format!("sqlite://{}", manager.path.display());
        let mut options = SqliteConnectOptions::from_str(&url).map_err(Error::sqlx)?;
        options = options
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = run_blocking(async move {
            SqlitePoolOptions::new()
                // Legacy rusqlite code assumes one transaction sticks to one
                // connection. Keep the compatibility layer serialized while
                // native SQLx repositories grow around it.
                .max_connections(1)
                .connect_with(options)
                .await
        })
        .map_err(Error::sqlx)?;
        Ok(Pool {
            inner: pool,
            _marker: PhantomData,
        })
    }
}

pub type PooledConnection<T = SqliteConnectionManager> = Connection<T>;

#[derive(Debug, Clone)]
pub struct Connection<T = SqliteConnectionManager> {
    pool: SqlitePool,
    tx: Option<SharedTx>,
    _marker: PhantomData<T>,
}

impl<T> Connection<T> {
    pub fn execute<P>(&self, sql: &str, params: P) -> Result<usize>
    where
        P: IntoParams,
    {
        match &self.tx {
            Some(tx) => execute_tx(tx.clone(), sql, params.into_params()),
            None => execute(&self.pool, sql, params.into_params()),
        }
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        match &self.tx {
            Some(tx) => execute_batch_tx(tx.clone(), sql),
            None => execute_batch(&self.pool, sql),
        }
    }

    pub fn query_row<P, F, TOut>(&self, sql: &str, params: P, mapper: F) -> Result<TOut>
    where
        P: IntoParams,
        F: FnOnce(&Row<'_>) -> Result<TOut>,
    {
        query_row(self, sql, params.into_params(), mapper)
    }

    pub fn prepare(&self, sql: &str) -> Result<Statement> {
        Ok(Statement {
            pool: self.pool.clone(),
            tx: self.tx.clone(),
            sql: sql.to_string(),
        })
    }

    pub fn prepare_cached(&self, sql: &str) -> Result<Statement> {
        self.prepare(sql)
    }

    pub fn transaction(&self) -> Result<Transaction<'_>> {
        let pool = self.pool.clone();
        let tx = run_blocking(async move { pool.begin().await }).map_err(Error::sqlx)?;
        let shared_tx = Arc::new(Mutex::new(Some(tx)));
        Ok(Transaction {
            conn: Connection {
                pool: self.pool.clone(),
                tx: Some(shared_tx.clone()),
                _marker: PhantomData,
            },
            tx: shared_tx,
            committed: false,
            _marker: PhantomData,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Statement {
    pool: SqlitePool,
    tx: Option<SharedTx>,
    sql: String,
}

impl Statement {
    pub fn query_row<P, F, TOut>(&mut self, params: P, mapper: F) -> Result<TOut>
    where
        P: IntoParams,
        F: FnOnce(&Row<'_>) -> Result<TOut>,
    {
        let conn = Connection::<SqliteConnectionManager> {
            pool: self.pool.clone(),
            tx: self.tx.clone(),
            _marker: PhantomData,
        };
        query_row(&conn, &self.sql, params.into_params(), mapper)
    }

    pub fn query_map<P, F, TOut>(&mut self, params: P, mut mapper: F) -> Result<MappedRows<TOut>>
    where
        P: IntoParams,
        F: FnMut(&Row<'_>) -> Result<TOut>,
    {
        let rows = match &self.tx {
            Some(tx) => fetch_all_tx(tx.clone(), &self.sql, params.into_params())?,
            None => fetch_all(&self.pool, &self.sql, params.into_params())?,
        };
        let mapped = rows
            .iter()
            .map(|inner| mapper(&Row { inner }))
            .collect::<Vec<_>>();
        Ok(MappedRows {
            inner: mapped.into_iter(),
        })
    }
}

pub struct MappedRows<T> {
    inner: std::vec::IntoIter<Result<T>>,
}

impl<T> Iterator for MappedRows<T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[derive(Debug)]
pub struct Transaction<'conn> {
    conn: Connection,
    tx: SharedTx,
    committed: bool,
    _marker: PhantomData<&'conn ()>,
}

impl Transaction<'_> {
    pub fn execute<P>(&self, sql: &str, params: P) -> Result<usize>
    where
        P: IntoParams,
    {
        self.conn.execute(sql, params)
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)
    }

    pub fn query_row<P, F, TOut>(&self, sql: &str, params: P, mapper: F) -> Result<TOut>
    where
        P: IntoParams,
        F: FnOnce(&Row<'_>) -> Result<TOut>,
    {
        self.conn.query_row(sql, params, mapper)
    }

    pub fn prepare(&self, sql: &str) -> Result<Statement> {
        self.conn.prepare(sql)
    }

    pub fn commit(mut self) -> Result<()> {
        let tx = self.tx.clone();
        run_blocking_local(move || {
            let mut guard = tx.lock().map_err(|err| Error::Message(err.to_string()))?;
            let Some(tx) = guard.take() else {
                return Err(Error::Message("transaction already closed".to_string()));
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("sqlite compatibility runtime")
                .block_on(async move { tx.commit().await.map_err(Error::sqlx) })
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Deref for Transaction<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let tx = self.tx.clone();
            let _ = run_blocking_local(move || {
                let mut guard = tx.lock().map_err(|err| Error::Message(err.to_string()))?;
                let Some(tx) = guard.take() else {
                    return Ok(());
                };
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("sqlite compatibility runtime")
                    .block_on(async move { tx.rollback().await.map_err(Error::sqlx) })
            });
        }
    }
}

pub struct Row<'r> {
    inner: &'r SqliteRow,
}

impl Row<'_> {
    pub fn get<I, T>(&self, idx: I) -> Result<T>
    where
        I: RowIndex,
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        idx.try_get(self.inner)
    }
}

pub trait RowIndex {
    fn try_get<T>(&self, row: &SqliteRow) -> Result<T>
    where
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>;
}

impl RowIndex for usize {
    fn try_get<T>(&self, row: &SqliteRow) -> Result<T>
    where
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        row.try_get::<T, _>(*self).map_err(Error::sqlx)
    }
}

impl RowIndex for i32 {
    fn try_get<T>(&self, row: &SqliteRow) -> Result<T>
    where
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        (*self as usize).try_get(row)
    }
}

impl RowIndex for &str {
    fn try_get<T>(&self, row: &SqliteRow) -> Result<T>
    where
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        row.try_get::<T, _>(*self).map_err(Error::sqlx)
    }
}

impl RowIndex for String {
    fn try_get<T>(&self, row: &SqliteRow) -> Result<T>
    where
        for<'a> T: sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        self.as_str().try_get(row)
    }
}

#[derive(Debug, Clone)]
pub enum SqlParam {
    Null,
    Text(String),
    I64(i64),
    F64(f64),
    Bool(bool),
    Bytes(Vec<u8>),
}

pub trait ToSql: Send + Sync {
    fn to_sql_param(&self) -> SqlParam;
}

impl<T> ToSql for &T
where
    T: ToSql + ?Sized,
{
    fn to_sql_param(&self) -> SqlParam {
        (*self).to_sql_param()
    }
}

impl<T> ToSql for Option<T>
where
    T: ToSql,
{
    fn to_sql_param(&self) -> SqlParam {
        self.as_ref()
            .map(ToSql::to_sql_param)
            .unwrap_or(SqlParam::Null)
    }
}

impl ToSql for str {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Text(self.to_string())
    }
}

impl ToSql for String {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Text(self.clone())
    }
}

impl ToSql for bool {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Bool(*self)
    }
}

impl ToSql for i64 {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::I64(*self)
    }
}

impl ToSql for i32 {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::I64(*self as i64)
    }
}

impl ToSql for u32 {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::I64(*self as i64)
    }
}

impl ToSql for usize {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::I64(*self as i64)
    }
}

impl ToSql for f64 {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::F64(*self)
    }
}

impl ToSql for Vec<u8> {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Bytes(self.clone())
    }
}

impl ToSql for [u8] {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Bytes(self.to_vec())
    }
}

impl ToSql for () {
    fn to_sql_param(&self) -> SqlParam {
        SqlParam::Null
    }
}

pub trait IntoParams {
    fn into_params(self) -> Vec<SqlParam>;
}

impl IntoParams for Vec<SqlParam> {
    fn into_params(self) -> Vec<SqlParam> {
        self
    }
}

impl IntoParams for &[SqlParam] {
    fn into_params(self) -> Vec<SqlParam> {
        self.to_vec()
    }
}

impl IntoParams for [(); 0] {
    fn into_params(self) -> Vec<SqlParam> {
        Vec::new()
    }
}

impl IntoParams for &[&dyn ToSql] {
    fn into_params(self) -> Vec<SqlParam> {
        self.iter().map(|value| value.to_sql_param()).collect()
    }
}

pub fn params_from_iter<I, T>(iter: I) -> Vec<SqlParam>
where
    I: IntoIterator<Item = T>,
    T: IntoSqlParam,
{
    iter.into_iter().map(IntoSqlParam::into_sql_param).collect()
}

pub trait IntoSqlParam {
    fn into_sql_param(self) -> SqlParam;
}

impl IntoSqlParam for SqlParam {
    fn into_sql_param(self) -> SqlParam {
        self
    }
}

impl IntoSqlParam for &dyn ToSql {
    fn into_sql_param(self) -> SqlParam {
        self.to_sql_param()
    }
}

impl IntoSqlParam for &&dyn ToSql {
    fn into_sql_param(self) -> SqlParam {
        (*self).to_sql_param()
    }
}

impl IntoSqlParam for &Box<dyn ToSql> {
    fn into_sql_param(self) -> SqlParam {
        self.as_ref().to_sql_param()
    }
}

pub trait OptionalExtension<T> {
    fn optional(self) -> Result<Option<T>>;
}

impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

#[macro_export]
macro_rules! params {
    () => {
        Vec::<$crate::sqlite_compat::SqlParam>::new()
    };
    ($($value:expr),+ $(,)?) => {{
        vec![$($crate::sqlite_compat::ToSql::to_sql_param(&$value)),+]
    }};
}

fn execute(pool: &SqlitePool, sql: &str, params: Vec<SqlParam>) -> Result<usize> {
    let pool = pool.clone();
    let sql = sql.to_string();
    run_blocking(async move {
        let result = bind_params(sqlx::query(&sql), params)
            .execute(&pool)
            .await?;
        Ok::<_, sqlx::Error>(result.rows_affected() as usize)
    })
    .map_err(Error::sqlx)
}

fn execute_batch(pool: &SqlitePool, sql: &str) -> Result<()> {
    let pool = pool.clone();
    let sql = sql.to_string();
    run_blocking(async move {
        sqlx::raw_sql(&sql).execute(&pool).await?;
        Ok::<_, sqlx::Error>(())
    })
    .map_err(Error::sqlx)
}

fn query_row<C, F, TOut>(
    conn: &Connection<C>,
    sql: &str,
    params: Vec<SqlParam>,
    mapper: F,
) -> Result<TOut>
where
    F: FnOnce(&Row<'_>) -> Result<TOut>,
{
    let mut rows = match &conn.tx {
        Some(tx) => fetch_all_tx(tx.clone(), sql, params)?,
        None => fetch_all(&conn.pool, sql, params)?,
    };
    if rows.is_empty() {
        return Err(Error::QueryReturnedNoRows);
    }
    let row = rows.remove(0);
    mapper(&Row { inner: &row })
}

fn fetch_all(pool: &SqlitePool, sql: &str, params: Vec<SqlParam>) -> Result<Vec<SqliteRow>> {
    let pool = pool.clone();
    let sql = sql.to_string();
    run_blocking(async move {
        bind_params(sqlx::query(&sql), params)
            .fetch_all(&pool)
            .await
    })
    .map_err(Error::sqlx)
}

fn execute_tx(tx: SharedTx, sql: &str, params: Vec<SqlParam>) -> Result<usize> {
    let sql = sql.to_string();
    run_blocking_local(move || {
        let mut guard = tx.lock().map_err(|err| Error::Message(err.to_string()))?;
        let Some(tx) = guard.as_mut() else {
            return Err(Error::Message("transaction already closed".to_string()));
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("sqlite compatibility runtime")
            .block_on(async move {
                let result = bind_params(sqlx::query(&sql), params)
                    .execute(&mut **tx)
                    .await
                    .map_err(Error::sqlx)?;
                Ok(result.rows_affected() as usize)
            })
    })
}

fn execute_batch_tx(tx: SharedTx, sql: &str) -> Result<()> {
    let sql = sql.to_string();
    run_blocking_local(move || {
        let mut guard = tx.lock().map_err(|err| Error::Message(err.to_string()))?;
        let Some(tx) = guard.as_mut() else {
            return Err(Error::Message("transaction already closed".to_string()));
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("sqlite compatibility runtime")
            .block_on(async move {
                sqlx::raw_sql(&sql)
                    .execute(&mut **tx)
                    .await
                    .map_err(Error::sqlx)?;
                Ok(())
            })
    })
}

fn fetch_all_tx(tx: SharedTx, sql: &str, params: Vec<SqlParam>) -> Result<Vec<SqliteRow>> {
    let sql = sql.to_string();
    run_blocking_local(move || {
        let mut guard = tx.lock().map_err(|err| Error::Message(err.to_string()))?;
        let Some(tx) = guard.as_mut() else {
            return Err(Error::Message("transaction already closed".to_string()));
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("sqlite compatibility runtime")
            .block_on(async move {
                bind_params(sqlx::query(&sql), params)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(Error::sqlx)
            })
    })
}

fn bind_params<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    params: Vec<SqlParam>,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for param in params {
        query = match param {
            SqlParam::Null => query.bind(Option::<String>::None),
            SqlParam::Text(value) => query.bind(value),
            SqlParam::I64(value) => query.bind(value),
            SqlParam::F64(value) => query.bind(value),
            SqlParam::Bool(value) => query.bind(value),
            SqlParam::Bytes(value) => query.bind(value),
        };
    }
    query
}

fn run_blocking<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("sqlite compatibility runtime")
            .block_on(future)
    })
    .join()
    .expect("sqlite compatibility thread")
}

fn run_blocking_local<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(f)
        .join()
        .expect("sqlite compatibility thread")
}
