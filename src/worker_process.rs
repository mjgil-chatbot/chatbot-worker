use std::any::Any;

pub fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

pub fn init_resilient_tracing(process_name: &str) -> Option<rust_debug::TracingGuard> {
    match std::panic::catch_unwind(|| rust_debug::init_tracing(process_name)) {
        Ok(Ok(guard)) => Some(guard),
        Ok(Err(err)) => {
            eprintln!("failed to initialize {process_name} tracing: {err}");
            None
        }
        Err(payload) => {
            eprintln!(
                "{process_name} tracing initialization panicked: {}",
                panic_payload_message(payload)
            );
            None
        }
    }
}
