use time::{format_description::FormatItem, macros::format_description, OffsetDateTime, UtcOffset};

const DISPLAY_TIMESTAMP_FORMAT: &[FormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

pub fn format_timestamp(value: OffsetDateTime) -> String {
    let value = UtcOffset::local_offset_at(value)
        .map(|offset| value.to_offset(offset))
        .unwrap_or(value);
    format_timestamp_value(value)
}

fn format_timestamp_value(value: OffsetDateTime) -> String {
    value
        .replace_nanosecond(0)
        .unwrap_or(value)
        .format(DISPLAY_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

pub fn format_duration(total_seconds: i64) -> String {
    let total_seconds = total_seconds.max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn format_timestamp_omits_subsecond_precision() {
        let timestamp = datetime!(2026-06-28 12:00:01.123456789 UTC);
        assert!(!format_timestamp(timestamp).contains('.'));
    }

    #[test]
    fn format_timestamp_value_keeps_only_date_and_time() {
        let timestamp = datetime!(2026-06-28 12:00:01.123456789 -4);
        assert_eq!(format_timestamp_value(timestamp), "2026-06-28 12:00:01");
    }
}
