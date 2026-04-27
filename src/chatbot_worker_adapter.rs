use std::path::Path;
use anyhow::Result;
fn is_user_stop_error(error: &str) -> bool {
    matches!(error, "Stopped by user" | "Cancelled by user")
}
