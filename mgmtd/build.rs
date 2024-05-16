use std::env;
use std::fmt::Write;
use std::path::Path;

fn main() {
    build_migrations();
}

fn build_migrations() {
    let mig_src_dir = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("src/db/schema");
    let mig_slice_file = Path::new(&env::var_os("OUT_DIR").unwrap()).join("migrations.slice");
    let current_schema_file = Path::new(&env::var_os("OUT_DIR").unwrap()).join("current.sql");

    let mut migrations = vec![];
    for e in std::fs::read_dir(&mig_src_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|dentry| dentry.file_type().is_ok_and(|ft| ft.is_file()))
    {
        let version: Option<u32> = e
            .path()
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok());

        if let Some(version) = version {
            migrations.push((version, std::fs::read_to_string(e.path()).unwrap()));
        }
    }

    migrations.sort_by_key(|(version, _)| *version);

    let base = migrations.iter().min().unwrap().0;
    let latest = migrations.iter().max().unwrap().0;
    if !migrations
        .iter()
        .map(|(version, _)| *version)
        .eq(base..=latest)
    {
        panic!(
            "Migration sequence {:?} is not contiguous",
            migrations
                .iter()
                .map(|(version, _)| *version)
                .collect::<Vec<u32>>()
        );
    }

    // Write migrations slice
    let mut migrations_slice = String::new();
    writeln!(migrations_slice, "&[").unwrap();
    for (version, sql) in &migrations {
        writeln!(
            migrations_slice,
            "Migration {{ version: {version}, sql: r#\"\n{sql}\"# }},"
        )
        .unwrap();
    }
    writeln!(migrations_slice, "]").unwrap();
    std::fs::write(mig_slice_file, migrations_slice).unwrap();

    // Test migrations and generate current schema
    let conn = rusqlite::Connection::open_in_memory().unwrap();

    for m in &migrations {
        conn.execute_batch(&m.1).unwrap();
    }

    let mut stmt = conn
        .prepare(
            "SELECT sql FROM sqlite_schema
            WHERE name NOT LIKE 'sqlite%'
            ORDER BY CASE
                WHEN type = 'table' THEN 0
                WHEN type = 'index' THEN 1
                WHEN type = 'view' THEN 2
                WHEN type = 'trigger' THEN 3
                ELSE 4
            END, name",
        )
        .unwrap();
    let mut rows = stmt.query([]).unwrap();

    let mut sql = String::new();
    while let Some(row) = rows.next().unwrap() {
        if let Ok(s) = row.get_ref_unwrap(0).as_str() {
            writeln!(sql, "{s};\n").unwrap();
        }
    }

    std::fs::write(current_schema_file, sql).unwrap();
    println!("cargo::rerun-if-changed={}", mig_src_dir.to_str().unwrap());
}
