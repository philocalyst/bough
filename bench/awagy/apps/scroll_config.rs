use std::sync::OnceLock;

pub fn row_count() -> usize {
    static ROW_COUNT: OnceLock<usize> = OnceLock::new();
    *ROW_COUNT.get_or_init(|| match std::env::var("SCROLL_ROWS") {
        Err(std::env::VarError::NotPresent) => 1000,
        value => match value.ok().and_then(|value| value.parse::<usize>().ok()) {
            Some(count) if count > 0 && count <= i32::MAX as usize => count,
            _ => {
                eprintln!(
                    "SCROLL_ROWS must be a positive integer no larger than {}",
                    i32::MAX
                );
                std::process::exit(1);
            }
        },
    })
}
