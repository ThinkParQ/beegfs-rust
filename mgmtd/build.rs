use std::env;
use std::path::Path;

fn main() {
    let mig_src_dir = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("src/db/schema");
    let migrations = sqlite::read_migrations(mig_src_dir).unwrap();

    let mig_slice_file = Path::new(&env::var_os("OUT_DIR").unwrap()).join("migrations.slice");
    let mig_slice_code = sqlite::migrations_slice_code(&migrations).unwrap();
    std::fs::write(mig_slice_file, mig_slice_code).unwrap();

    let current_schema_file = Path::new(&env::var_os("OUT_DIR").unwrap()).join("current.sql");
    let schema_sql = sqlite::flatten_migrations(&migrations).unwrap();
    std::fs::write(current_schema_file, schema_sql).unwrap();
}
