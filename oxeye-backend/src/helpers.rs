use rand::distr::{Alphanumeric, SampleString};
use rand::rng;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn generate_code() -> String {
  format!("oxeye-{}", Alphanumeric.sample_string(&mut rng(), 6))
}

pub fn generate_api_key() -> String {
  format!("oxeye-sk-{}", Alphanumeric.sample_string(&mut rng(), 32))
}

pub fn hash_api_key(key: &str) -> String {
  format!("{:x}", Sha256::digest(key.as_bytes()))
}

pub fn now() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64
}

/// Format time online based on duration.
/// - Show seconds if < 1 minute
/// - Show minutes if < 1 hour
/// - Show hours if < 24 hours
/// - Show days if >= 24 hours
pub fn format_time_online(duration_secs: i64) -> String {
  const MINUTE: i64 = 60;
  const HOUR: i64 = 60 * MINUTE;
  const DAY: i64 = 24 * HOUR;

  if duration_secs < MINUTE {
    format!("{} seconds", duration_secs)
  } else if duration_secs < HOUR {
    let minutes = duration_secs / MINUTE;
    format!("{} minutes", minutes)
  } else if duration_secs < DAY {
    let hours = duration_secs / HOUR;
    format!("{} hours", hours)
  } else {
    let days = duration_secs / DAY;
    format!("{} days", days)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_format_time_online_zero_seconds() {
    assert_eq!(format_time_online(0), "0 seconds");
  }

  #[test]
  fn test_format_time_online_seconds() {
    assert_eq!(format_time_online(1), "1 seconds");
    assert_eq!(format_time_online(30), "30 seconds");
    assert_eq!(format_time_online(59), "59 seconds");
  }

  #[test]
  fn test_format_time_online_minutes() {
    assert_eq!(format_time_online(60), "1 minutes");
    assert_eq!(format_time_online(90), "1 minutes"); // 1 minute 30 seconds -> 1 minutes
    assert_eq!(format_time_online(120), "2 minutes");
    assert_eq!(format_time_online(1800), "30 minutes");
    assert_eq!(format_time_online(3599), "59 minutes"); // 59 minutes 59 seconds -> 59 minutes
  }

  #[test]
  fn test_format_time_online_hours() {
    assert_eq!(format_time_online(3600), "1 hours");
    assert_eq!(format_time_online(5400), "1 hours"); // 1 hour 30 minutes -> 1 hours
    assert_eq!(format_time_online(7200), "2 hours");
    assert_eq!(format_time_online(43200), "12 hours");
    assert_eq!(format_time_online(86399), "23 hours"); // 23 hours 59 minutes -> 23 hours
  }

  #[test]
  fn test_format_time_online_days() {
    assert_eq!(format_time_online(86400), "1 days");
    assert_eq!(format_time_online(129600), "1 days"); // 1 day 12 hours -> 1 days
    assert_eq!(format_time_online(172800), "2 days");
    assert_eq!(format_time_online(604800), "7 days");
    assert_eq!(format_time_online(2592000), "30 days");
  }

  #[test]
  fn test_format_time_online_boundary_cases() {
    // Test exact boundaries
    assert_eq!(format_time_online(59), "59 seconds"); // Just before 1 minute
    assert_eq!(format_time_online(60), "1 minutes"); // Exactly 1 minute
    assert_eq!(format_time_online(3599), "59 minutes"); // Just before 1 hour
    assert_eq!(format_time_online(3600), "1 hours"); // Exactly 1 hour
    assert_eq!(format_time_online(86399), "23 hours"); // Just before 1 day
    assert_eq!(format_time_online(86400), "1 days"); // Exactly 1 day
  }
}
