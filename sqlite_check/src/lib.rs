use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use syn::{LitStr, parse_macro_input};

/// The global connection handle
static DB_CONN: OnceLock<Arc<Mutex<rusqlite::Connection>>> = OnceLock::new();

/// Takes a string literal and executes it against the Managements SQLite database, checking it for
/// validity. This includes the general SQL syntax as well as the provided field and table names.
/// If something is wrong, compilation will fail. Can **not** be used for dynamically built
/// queries.
///
/// Returns the same string for further processing (e.g. using it for the actual query).
///
/// The macro will only create one global in-memory database and fill it with the management schema
/// as well as the test data. The connection handle is then reused for further checks. The given
/// statement is only prepared, not actually executed. Therefore, one single test database can be
/// reused for all checks in the whole project.
///
/// # Example
/// Valid SQL:
/// ```ignore
/// use sql_check::sql;
///
/// let query = sql!("SELECT * FROM nodes");
/// assert_eq!(query, "SELECT * FROM nodes");
/// ```
///
/// Invalid SQL:
/// ```ignore
/// use sql_check::sql;
///
/// let query = sql!("SELECT nonexisting_field FROM nodes"); // panic!
/// ```
#[proc_macro]
pub fn sql(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse the input
    let orig_input = input.clone();
    let input = parse_macro_input!(input as LitStr).value();

    let result = {
        // Get the global connection handle
        let conn = DB_CONN.get_or_init(open_db).lock().unwrap();
        // Prepare the statement
        conn.prepare(&input).map(|_| {})
    };

    if let Err(err) = result {
        panic!("SQL statement is invalid: {err}");
    }

    orig_input
}

/// Set up an in memory SQLite database for testing
fn open_db() -> Arc<Mutex<rusqlite::Connection>> {
    let conn = sqlite::open_in_memory().unwrap();
    let schema_file = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("current.sql");

    conn.execute_batch(&std::fs::read_to_string(schema_file).unwrap())
        .unwrap();

    Arc::new(Mutex::new(conn))
}
