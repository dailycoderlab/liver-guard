//! Prints the JSON Schema of the script document; the desktop UI derives its
//! TypeScript types from it (`pnpm gen:types`), so both sides never drift.
fn main() {
    println!("{}", serde_json::to_string_pretty(&liverguard_ir::json_schema()).expect("schema serializes"));
}
