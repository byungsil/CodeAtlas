use std::env;
use rusqlite::{params, Connection, OpenFlags, Result};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --example query_db -- <db-path> [qualified-name] [search-name] [mode]");
        std::process::exit(1);
    }

    let db_path = &args[1];
    let qualified_name = args.get(2).map(|s| s.as_str()).unwrap_or("cv::Mat");
    let search_name = args.get(3).map(|s| s.as_str()).unwrap_or("Mat");
    let mode = args.get(4).map(|s| s.as_str()).unwrap_or("rw");

    let conn = match mode {
        "ro" => Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        _ => Connection::open(db_path)?,
    };

    let symbol_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
    let call_count: i64 = conn.query_row("SELECT COUNT(*) FROM calls", [], |row| row.get(0))?;
    println!("symbols={symbol_count} calls={call_count}");

    let mut stmt = conn.prepare(
        "SELECT id, qualified_name, type, file_path, line
         FROM symbols
         WHERE qualified_name = ?1",
    )?;
    let mut rows = stmt.query(params![qualified_name])?;
    if let Some(row) = rows.next()? {
        let symbol_id: String = row.get(0)?;
        let qualified_name: String = row.get(1)?;
        let symbol_type: String = row.get(2)?;
        let file_path: String = row.get(3)?;
        let line: i64 = row.get(4)?;
        println!("exact={qualified_name} | {symbol_type} | {file_path}:{line}");

        let caller_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE callee_id = ?1",
            params![symbol_id.as_str()],
            |r| r.get(0),
        )?;
        let callee_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE caller_id = ?1",
            params![symbol_id.as_str()],
            |r| r.get(0),
        )?;
        println!("exact_callers={caller_count} exact_callees={callee_count}");
    } else {
        println!("exact=not-found");
    }

    let mut search = conn.prepare(
        "SELECT qualified_name, type, file_path, line
         FROM symbols
         WHERE name = ?1
         ORDER BY qualified_name
         LIMIT 10",
    )?;
    let search_rows = search.query_map(params![search_name], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    println!("search_results:");
    for row in search_rows {
        let (qualified_name, symbol_type, file_path, line) = row?;
        println!("  {qualified_name} | {symbol_type} | {file_path}:{line}");
    }

    Ok(())
}
