use anyhow::{bail, Context, Result};
use std::fmt::Write;
use std::path::Path;

/// Represents a migration step using a static SQL string. A slice of these is generated as Rust
/// code and stored to disk by write_migrations_file() to be read in at runtime.
#[derive(Debug)]
pub struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// Represents a migration step using an owned SQL string. Used when reading in the migrations from
/// .sql files.
#[derive(Debug)]
pub struct OwnedMigration {
    pub version: u32,
    pub sql: String,
}

/// Reads in migration files from the given source directory.
///
/// Migration filed must be named n.sql where n is a u32 integer. The first file should be 1.sql,
/// further ones must each increment by 1. The sequence must be contiguous, although it's not
/// required to start at 1. This allows to drop the earlier ones at some point. When doing that,
/// the oldest remaining migration must be modified to contain the new initial migration.
///
/// This function is meant to be run from a build script.
pub fn read_migrations(src_dir: impl AsRef<Path>) -> Result<Vec<OwnedMigration>> {
    let mut migrations = vec![];

    // Go through the migration files, ignoring all others
    for file in std::fs::read_dir(src_dir)?
        .filter_map(Result::ok)
        .filter(|dentry| dentry.file_type().is_ok_and(|ft| ft.is_file()))
        .map(|dentry| dentry.path())
    {
        let version: Option<u32> = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok());

        if let Some(version) = version {
            migrations.push(OwnedMigration {
                version,
                sql: std::fs::read_to_string(file)?,
            });
        }
    }

    if migrations.is_empty() {
        bail!("No migrations found");
    }

    // Migration order is important
    migrations.sort_by_key(|mig| mig.version);

    let base = migrations.iter().min_by_key(|mig| mig.version).unwrap();
    let latest = migrations.iter().max_by_key(|mig| mig.version).unwrap();

    if !migrations
        .iter()
        .map(|mig| mig.version)
        .eq(base.version..=latest.version)
    {
        bail!(
            "Migration sequence {:?} is not contiguous",
            migrations
                .iter()
                .map(|mig| mig.version)
                .collect::<Vec<u32>>()
        );
    }

    Ok(migrations)
}

/// Generates Rust code that defines a slice of type `&[::sqlite:Migration]` from the given
/// migration list.
///
/// The slice can be written to disk (e.g. by the build script) and read from disk into a constant
/// like this:
/// ```ignore
/// const MIGRATIONS: &[sqlite::Migration] = include_str!("migrations.slice");
/// ```
///
/// This function is meant to be run from a build script.
pub fn migrations_slice_code(migrations: &[OwnedMigration]) -> Result<String> {
    let mut migrations_slice = String::new();

    writeln!(migrations_slice, "&[")?;
    for OwnedMigration { version, sql } in migrations {
        writeln!(
            migrations_slice,
            "::sqlite::Migration {{ version: {version}, sql: r#\"\n{sql}\"# }},"
        )?;
    }
    writeln!(migrations_slice, "]")?;

    Ok(migrations_slice)
}

/// Applies the given migrations to a temporary database and generates an SQL schema string from it
/// - representing the latest version of the schema.
///
/// This function is meant to be run from a build script.
pub fn flatten_migrations(migrations: &[OwnedMigration]) -> Result<String> {
    let conn = rusqlite::Connection::open_in_memory()?;

    if migrations.is_empty() {
        bail!("No migrations given");
    }

    let base = migrations.first().unwrap().version;
    let latest = migrations.last().unwrap().version;
    if !migrations.iter().map(|mig| mig.version).eq(base..=latest) {
        bail!(
            "Migration sequence {:?} is not contiguous",
            migrations
                .iter()
                .map(|mig| mig.version)
                .collect::<Vec<u32>>()
        );
    }

    for m in migrations {
        conn.execute_batch(&m.sql)?;
    }

    // The order of the SQL statements is important as generating views or triggers will fail if
    // the corresponding tables don't exist yet.
    let mut stmt = conn.prepare(
        "SELECT sql FROM sqlite_schema
        WHERE name NOT LIKE 'sqlite%'
        ORDER BY CASE
            WHEN type = 'table' THEN 0
            WHEN type = 'index' THEN 1
            WHEN type = 'view' THEN 2
            WHEN type = 'trigger' THEN 3
            ELSE 4
        END, name",
    )?;
    let mut rows = stmt.query([])?;

    let mut sql = String::new();
    while let Some(row) = rows.next()? {
        if let Ok(s) = row.get_ref(0)?.as_str() {
            writeln!(sql, "{s};\n")?;
        }
    }

    Ok(sql)
}

/// Migrates a database to the latest version using the given migration list.
///
/// This function is meant to be called at runtime to upgrade the database.
pub fn migrate_schema(conn: &mut rusqlite::Connection, migrations: &[Migration]) -> Result<()> {
    let tx = conn.transaction()?;

    if migrations.is_empty() {
        bail!("No migrations given");
    }

    let base = migrations.first().unwrap().version;
    let latest = migrations.last().unwrap().version;
    if !migrations.iter().map(|mig| mig.version).eq(base..=latest) {
        bail!(
            "Migration sequence {:?} is not contiguous",
            migrations
                .iter()
                .map(|mig| mig.version)
                .collect::<Vec<u32>>()
        );
    }

    // The databases version is stored in this special sqlite header variable
    let mut version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version == 0 {
        println!("New database, migrating to latest version {latest}");
    } else if (base..latest).contains(&version) {
        println!("Database has version {version}, migrating to latest version {latest}");
    } else if version == latest {
        bail!("Database schema is up to date (version {version})");
    } else {
        bail!(
            "Database schema version {version} is outside of the valid range ({} to {})",
            base,
            latest
        )
    };

    // Since the base migration is the starting point for new databases, a new database version can
    // be handled like the version before the current base
    if version == 0 {
        version = base - 1;
    }

    // Apply the migrations
    for Migration { version, sql } in migrations.iter().skip((1 + version - base) as usize) {
        tx.execute_batch(sql)
            .with_context(|| format!("Database migration {version} failed"))?;
    }

    // update the database version to the latest schema version
    tx.pragma_update(None, "user_version", latest)?;

    tx.commit()?;

    println!("Database schema successfully migrated to version {latest}");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn migrate_schema() {
        let mut conn = crate::connection::open_in_memory().unwrap();

        let mut migrations = vec![Migration {
            version: 1,
            sql: "CREATE TABLE t1 (id INTEGER)",
        }];
        super::migrate_schema(&mut conn, &migrations).unwrap();

        migrations.push(Migration {
            version: 2,
            sql: "CREATE TABLE t2 (id INTEGER)",
        });
        super::migrate_schema(&mut conn, &migrations).unwrap();

        migrations.push(Migration {
            version: 3,
            sql: "CREATE TABLE t3 (id INTEGER)",
        });
        migrations.push(Migration {
            version: 4,
            sql: "CREATE TABLE t4 (id INTEGER)",
        });
        super::migrate_schema(&mut conn, &migrations).unwrap();

        let mut migrations = migrations.split_off(3);
        migrations.push(Migration {
            version: 5,
            sql: "DROP TABLE t1",
        });
        super::migrate_schema(&mut conn, &migrations).unwrap();

        let version = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap();

        assert_eq!(5, version);

        let tables = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type == 'table' AND name LIKE 't%'",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap();

        assert_eq!(3, tables);

        // Failure on up-to-date db
        super::migrate_schema(&mut conn, &migrations).unwrap_err();

        // Failure on non-contiguous migration sequence
        migrations.push(Migration {
            version: 7,
            sql: "CREATE TABLE t7 (id INTEGER)",
        });
        migrations.push(Migration {
            version: 6,
            sql: "CREATE TABLE t6 (id INTEGER)",
        });
        super::migrate_schema(&mut conn, &migrations).unwrap_err();
    }

    #[test]
    fn flatten_migrations() {
        let migrations = &[
            OwnedMigration {
                version: 1,
                sql: "CREATE TABLE t1 (id INTEGER)".to_string(),
            },
            OwnedMigration {
                version: 2,
                sql: "CREATE TABLE t2 (id INTEGER)".to_string(),
            },
            OwnedMigration {
                version: 3,
                sql: "ALTER TABLE t1 ADD c2 INTEGER".to_string(),
            },
            OwnedMigration {
                version: 4,
                sql: "DROP TABLE t2".to_string(),
            },
            OwnedMigration {
                version: 5,
                sql: "CREATE VIEW v AS SELECT * FROM t1".to_string(),
            },
            OwnedMigration {
                version: 6,
                sql: "CREATE INDEX i ON t1(id)".to_string(),
            },
        ];

        let sql = super::flatten_migrations(migrations).unwrap();

        assert_eq!(
            "CREATE TABLE t1 (id INTEGER, c2 INTEGER);

CREATE INDEX i ON t1(id);

CREATE VIEW v AS SELECT * FROM t1;",
            sql.trim()
        );
    }
}
