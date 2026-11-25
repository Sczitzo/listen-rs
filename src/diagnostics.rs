use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn write_report(path: &Path, event: &str, detail: &str) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let report = format!(
        "Listen {}\nTime (Unix seconds): {time}\nProcess: {}\nEvent: {event}\n{detail}\n",
        env!("CARGO_PKG_VERSION"), std::process::id()
    );
    crate::storage::atomic_write(path, report.as_bytes())
}

pub(crate) fn install_crash_report(profile: &Path) {
    let path = profile.join("last-crash.txt");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Panic messages may quote user input. Keep only the source location and call stack.
        let source = info.location().map_or_else(|| "unknown".into(), |location| location.to_string());
        let detail = format!("Source: {source}\n{}", std::backtrace::Backtrace::force_capture());
        if let Err(error) = write_report(&path, "Unexpected Rust error", &detail) {
            eprintln!("Could not write Listen's crash report: {:?}", error.kind());
        }
        previous(info);
    }));
}

pub(crate) fn error(event: &str) {
    error_at(&crate::storage::data_dir(), event);
}

pub(crate) fn error_at(profile: &Path, event: &str) {
    let path = profile.join("last-error.txt");
    if let Err(error) = write_report(&path, event, "") {
        eprintln!("Could not write Listen's error report: {:?}", error.kind());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_replaces_the_previous_error_and_has_a_date_and_version() {
        let path = std::env::temp_dir().join(format!("listen-report-{}.txt", std::process::id()));
        write_report(&path, "Previous error", "").unwrap();
        write_report(&path, "Latest error", "").unwrap();
        let report = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(report.contains(env!("CARGO_PKG_VERSION")));
        assert!(report.contains("Time (Unix seconds):"));
        assert!(report.contains("Latest error"));
        assert!(!report.contains("Previous error"));
    }
}
