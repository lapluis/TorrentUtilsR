pub fn human_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    const GB: usize = 1024 * MB;

    if bytes >= GB {
        let whole = bytes / GB;
        let remainder = bytes % GB;
        if remainder == 0 {
            format!("{whole} GiB")
        } else {
            let value = bytes as f64 / GB as f64;
            format!("{value:.2} GiB")
        }
    } else if bytes >= MB {
        let whole = bytes / MB;
        let remainder = bytes % MB;
        if remainder == 0 {
            format!("{whole} MiB")
        } else {
            let value = bytes as f64 / MB as f64;
            format!("{value:.2} MiB")
        }
    } else if bytes >= KB {
        let whole = bytes / KB;
        let remainder = bytes % KB;
        if remainder == 0 {
            format!("{whole} KiB")
        } else {
            let value = bytes as f64 / KB as f64;
            format!("{value:.2} KiB")
        }
    } else {
        format!("{bytes} B")
    }
}
