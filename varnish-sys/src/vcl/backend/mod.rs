mod backend_main;
mod backend_ref;
mod director;

pub use backend_main::*;
pub use backend_ref::{BackendRef, ProbeResult};
pub use director::{Director, VclDirector};

/// Creates a JSON string with custom indentation and writes it to a Buffer:
/// - Top level line has no indentation
/// - All other lines have 6 extra spaces on top of normal indentation
/// - Appends a trailing comma and newline with indentation
///
/// Usage: `report_details_json!(vsb, serde_json::json!({ "key": "value" }))`
#[macro_export]
macro_rules! report_details_json {
    ($vsb:expr, $json_value:expr) => {{
        let json_str =
            serde_json::to_string_pretty(&$json_value).expect("Failed to serialize JSON");

        let indent = "      "; // 6 spaces
        let lines: Vec<&str> = json_str.lines().collect();
        if let Some((first, rest)) = lines.split_first() {
            let _ = $vsb.write(first);
            for line in rest {
                let _ = $vsb.write(&"\n");
                let _ = $vsb.write(&indent);
                let _ = $vsb.write(line);
            }
        } else {
            let _ = $vsb.write(&json_str);
        }
        let _ = $vsb.write(&",\n");
        let _ = $vsb.write(&indent);
    }};
}
