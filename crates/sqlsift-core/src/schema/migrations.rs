//! Support for migration tool conventions: rollback (down/undo) files and the
//! down sections of dbmate, goose and sql-migrate files.
//!
//! Only the "up" direction describes the schema, so rollback SQL must not be
//! applied when building the catalog.

use std::borrow::Cow;
use std::path::Path;

/// Whether `path` names a rollback migration that should be skipped when
/// loading a directory of migrations:
///
/// - golang-migrate / sqlx reversible migrations: `*.down.sql`
/// - Flyway undo migrations: `U<version>__<description>.sql`
///
/// ```
/// use std::path::Path;
/// use sqlsift_core::schema::is_rollback_migration;
///
/// assert!(is_rollback_migration(Path::new("000001_init.down.sql")));
/// assert!(is_rollback_migration(Path::new("U1__init.sql")));
/// assert!(!is_rollback_migration(Path::new("V1__init.sql")));
/// ```
pub fn is_rollback_migration(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".down.sql") {
        return true;
    }
    is_flyway_undo(name)
}

/// `U` followed by a version (digits, `.` or `_`, starting with a digit) and `__`
fn is_flyway_undo(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('U') else {
        return false;
    };
    let Some((version, _)) = rest.split_once("__") else {
        return false;
    };
    version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
}

/// Natural sort key for migration files.
///
/// Compares the parent directory first so subdirectories are processed in
/// order without interleaving versions across directories, then compares the
/// leading version numerically:
/// - Flyway migrations: `V<version>__<description>.sql`, where the version may
///   be dotted (`1.2.3`) or underscore-separated (`1_2`), comparing each numeric segment.
/// - Other migration conventions (goose, golang-migrate, timestamps): the leading ASCII digits.
///
/// Files with a version precede unversioned files within the same directory.
/// Files with equal versions or no version fall back to lexical path comparison.
///
/// ```
/// use std::path::Path;
/// use sqlsift_core::schema::migration_sort_key;
///
/// assert!(migration_sort_key(Path::new("fly/V1__init.sql")) < migration_sort_key(Path::new("fly/V2__r.sql")));
/// assert!(migration_sort_key(Path::new("fly/V2__r.sql")) < migration_sort_key(Path::new("fly/V10__r.sql")));
/// assert!(migration_sort_key(Path::new("1_init.sql")) < migration_sort_key(Path::new("10_r.sql")));
/// assert!(migration_sort_key(Path::new("a_tables/2_orders.sql")) < migration_sort_key(Path::new("b_alter/1_note.sql")));
/// ```
pub fn migration_sort_key(path: &Path) -> (&Path, bool, Option<Vec<u64>>, &Path) {
    let parent = path.parent().unwrap_or(Path::new(""));
    let version = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(parse_migration_version);
    (parent, version.is_none(), version, path)
}

/// Sort migration files by their natural version key.
pub fn sort_migration_files<P: AsRef<Path>>(files: &mut [P]) {
    files.sort_by(|a, b| migration_sort_key(a.as_ref()).cmp(&migration_sort_key(b.as_ref())));
}

fn parse_migration_version(name: &str) -> Option<Vec<u64>> {
    // 1. Flyway version: V<version>__<description>.sql
    if let Some(rest) = name.strip_prefix('V').or_else(|| name.strip_prefix('v')) {
        if let Some((version_str, _)) = rest.split_once("__") {
            if version_str.starts_with(|c: char| c.is_ascii_digit()) {
                let parts: Result<Vec<u64>, _> = version_str
                    .split(['.', '_'])
                    .map(str::parse::<u64>)
                    .collect();
                if let Ok(parts) = parts {
                    if !parts.is_empty() {
                        return Some(parts);
                    }
                }
            }
        }
    }

    // 2. Otherwise leading digits (e.g. goose / golang-migrate: 1_init.sql, 002_alter.sql,
    //    timestamp 20261012120000_create.sql)
    let digits_len = name.chars().take_while(char::is_ascii_digit).count();
    if digits_len > 0 {
        if let Ok(num) = name[..digits_len].parse::<u64>() {
            return Some(vec![num]);
        }
    }

    None
}

/// Blank out the "down" sections of migration files that hold both directions:
/// every line after a down marker, up to the next up marker (or the end of the
/// file). The markers are those of
///
/// - dbmate: `-- migrate:down` / `-- migrate:up`
/// - goose: `-- +goose Down` / `-- +goose Up`
/// - sql-migrate: `-- +migrate Down` / `-- +migrate Up`
///
/// Removed text is replaced with spaces (newlines are kept), so byte offsets
/// and line numbers of the remaining statements are unchanged. Input without
/// a down marker is returned as is.
///
/// ```
/// use sqlsift_core::schema::strip_down_migrations;
///
/// let sql = "-- +goose Up\nCREATE TABLE t (id INT);\n-- +goose Down\nDROP TABLE t;\n";
/// assert_eq!(
///     strip_down_migrations(sql),
///     "-- +goose Up\nCREATE TABLE t (id INT);\n              \n             \n"
/// );
/// ```
pub fn strip_down_migrations(sql: &str) -> Cow<'_, str> {
    let has_down = sql.contains("migrate:down") || {
        let lower = sql.to_ascii_lowercase();
        lower.contains("+goose down") || lower.contains("+migrate down")
    };
    if !has_down {
        return Cow::Borrowed(sql);
    }

    let mut out = String::with_capacity(sql.len());
    let mut in_down = false;
    for line in sql.split_inclusive('\n') {
        match migrate_marker(line) {
            Some(Marker::Down) => in_down = true,
            Some(Marker::Up) => in_down = false,
            None => {}
        }
        if in_down {
            out.extend(line.bytes().map(|b| if b == b'\n' { '\n' } else { ' ' }));
        } else {
            out.push_str(line);
        }
    }
    Cow::Owned(out)
}

enum Marker {
    Up,
    Down,
}

/// Recognize a marker line: dbmate's `-- migrate:up` or `-- migrate:down`,
/// optionally followed by options such as `transaction:false`, or goose's and
/// sql-migrate's `-- +goose Up` / `-- +migrate Down` (any case, optionally
/// followed by options such as `notransaction`).
fn migrate_marker(line: &str) -> Option<Marker> {
    let rest = line.trim_start().strip_prefix("--")?.trim_start();
    if let Some(rest) = rest.strip_prefix('+') {
        let (tool, rest) = rest.split_once(char::is_whitespace)?;
        if !(tool.eq_ignore_ascii_case("goose") || tool.eq_ignore_ascii_case("migrate")) {
            return None;
        }
        let direction = rest.split_whitespace().next()?;
        return if direction.eq_ignore_ascii_case("up") {
            Some(Marker::Up)
        } else if direction.eq_ignore_ascii_case("down") {
            Some(Marker::Down)
        } else {
            None
        };
    }
    let rest = rest.strip_prefix("migrate:")?;
    let (marker, after) = match rest.strip_prefix("down") {
        Some(after) => (Marker::Down, after),
        None => (Marker::Up, rest.strip_prefix("up")?),
    };
    // Reject e.g. `migrate:downstream`
    (after.is_empty() || after.starts_with(char::is_whitespace)).then_some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sorts_flyway_migrations_in_natural_numeric_order() {
        let mut files = vec![
            PathBuf::from("fly/V10__r.sql"),
            PathBuf::from("fly/V2__r.sql"),
            PathBuf::from("fly/V1__init.sql"),
            PathBuf::from("fly/V1.2.3__patch.sql"),
            PathBuf::from("fly/V1.2__minor.sql"),
            PathBuf::from("fly/V1_1__under.sql"),
        ];
        sort_migration_files(&mut files);
        assert_eq!(
            files,
            vec![
                PathBuf::from("fly/V1__init.sql"),
                PathBuf::from("fly/V1_1__under.sql"),
                PathBuf::from("fly/V1.2__minor.sql"),
                PathBuf::from("fly/V1.2.3__patch.sql"),
                PathBuf::from("fly/V2__r.sql"),
                PathBuf::from("fly/V10__r.sql"),
            ]
        );
    }

    #[test]
    fn sorts_unpadded_leading_digits_numerically() {
        let mut files = vec![
            PathBuf::from("migrations/10_tenth.sql"),
            PathBuf::from("migrations/2_second.sql"),
            PathBuf::from("migrations/1_first.sql"),
        ];
        sort_migration_files(&mut files);
        assert_eq!(
            files,
            vec![
                PathBuf::from("migrations/1_first.sql"),
                PathBuf::from("migrations/2_second.sql"),
                PathBuf::from("migrations/10_tenth.sql"),
            ]
        );
    }

    #[test]
    fn versioned_files_precede_unversioned_files() {
        let mut files = vec![
            PathBuf::from("schema/views.sql"),
            PathBuf::from("schema/V1__init.sql"),
            PathBuf::from("schema/02_orders.sql"),
            PathBuf::from("schema/functions.sql"),
        ];
        sort_migration_files(&mut files);
        assert_eq!(
            files,
            vec![
                PathBuf::from("schema/V1__init.sql"),
                PathBuf::from("schema/02_orders.sql"),
                PathBuf::from("schema/functions.sql"),
                PathBuf::from("schema/views.sql"),
            ]
        );
    }

    #[test]
    fn preserves_directory_order_across_subdirectories() {
        let mut files = vec![
            PathBuf::from("db/b_alter/1_note.sql"),
            PathBuf::from("db/a_tables/2_orders.sql"),
            PathBuf::from("db/a_tables/10_items.sql"),
            PathBuf::from("db/a_tables/1_users.sql"),
        ];
        sort_migration_files(&mut files);
        assert_eq!(
            files,
            vec![
                PathBuf::from("db/a_tables/1_users.sql"),
                PathBuf::from("db/a_tables/2_orders.sql"),
                PathBuf::from("db/a_tables/10_items.sql"),
                PathBuf::from("db/b_alter/1_note.sql"),
            ]
        );
    }
}
