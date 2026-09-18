//! Per-test Postgres databases for the `db-tests` suite.
//!
//! [`with_pool`] creates a fresh database on the server named by
//! `DATABASE_URL` (read from the environment or `.env`), runs the test body
//! against a pool for it, and drops the database again once the body returns.
//! A body that panics leaves its database behind; those leftovers are swept
//! the next time a test process starts.

use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool, postgres::PgPoolOptions};
use tokio::sync::OnceCell;

/// Prefix shared by every database this module creates.
const PREFIX: &str = "eks_test_";

/// Per-process counter that keeps concurrently running tests apart.
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Guards the one-time sweep of databases left behind by earlier runs.
static SWEPT: OnceCell<()> = OnceCell::const_new();

/// Run `body` against a pool for a freshly created, empty database.
///
/// The database is dropped after `body` returns (whatever it returns), and its
/// result is passed through unchanged.
pub async fn with_pool<F, Fut, T>(body: F) -> T
where
    F: FnOnce(PgPool) -> Fut,
    Fut: Future<Output = T>,
{
    let admin_url = dotenvy::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a Postgres server to run database tests");
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect to DATABASE_URL");

    SWEPT.get_or_init(|| sweep_stale(&mut admin)).await;

    let name = format!(
        "{PREFIX}{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    // `name` is built from a fixed prefix, the process id and a counter, so it
    // cannot carry anything but `[a-z0-9_]`.
    sqlx::query(AssertSqlSafe(format!(r#"CREATE DATABASE "{name}""#)))
        .execute(&mut admin)
        .await
        .expect("create test database");

    let mut test_url = url::Url::parse(&admin_url).expect("DATABASE_URL is a valid URL");
    test_url.set_path(&format!("/{name}"));
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(test_url.as_str())
        .await
        .expect("connect to test database");

    let outcome = body(pool.clone()).await;

    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#
    )))
    .execute(&mut admin)
    .await
    .expect("drop test database");

    outcome
}

/// Drop databases created by earlier test processes (a panicking test skips
/// its own cleanup). Databases of the current process are left alone.
async fn sweep_stale(admin: &mut PgConnection) {
    let own = format!("{PREFIX}{}_", std::process::id());
    let stale: Vec<(String,)> =
        sqlx::query_as("SELECT datname FROM pg_database WHERE datname LIKE $1")
            .bind(format!("{PREFIX}%"))
            .fetch_all(&mut *admin)
            .await
            .expect("list test databases");

    for (name,) in stale {
        if name.starts_with(&own) {
            continue;
        }
        // `name` came back from `pg_database` filtered on our prefix; quoting it
        // as an identifier keeps any unexpected character inert.
        sqlx::query(AssertSqlSafe(format!(
            r#"DROP DATABASE IF EXISTS "{}" WITH (FORCE)"#,
            name.replace('"', "\"\"")
        )))
        .execute(&mut *admin)
        .await
        .expect("drop stale test database");
    }
}
