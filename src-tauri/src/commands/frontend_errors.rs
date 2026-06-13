use crate::domain::AppState;
use crate::{append_text_file, redact_sensitive_text, truncate_text};
use chrono::Utc;
use tauri::State;

#[tauri::command]
pub(crate) fn record_frontend_error(
    message: String,
    stack: Option<String>,
    component_stack: Option<String>,
    state: State<AppState>,
) -> Result<(), String> {
    let content =
        format_frontend_error_entry(&message, stack.as_deref(), component_stack.as_deref());
    append_text_file(&state.app_dir.join("frontend-errors.log"), &content)
}

pub(crate) fn format_frontend_error_entry(
    message: &str,
    stack: Option<&str>,
    component_stack: Option<&str>,
) -> String {
    let message = redact_sensitive_text(&truncate_text(message, 4_000));
    let stack = redact_sensitive_text(&truncate_text(stack.unwrap_or(""), 12_000));
    let component_stack =
        redact_sensitive_text(&truncate_text(component_stack.unwrap_or(""), 12_000));
    format!(
        "[{}] Frontend render error\nMessage: {}\nStack:\n{}\nComponent stack:\n{}\n\n",
        Utc::now().to_rfc3339(),
        message,
        stack,
        component_stack
    )
}
