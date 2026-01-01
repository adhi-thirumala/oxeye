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
    assert_eq!(format_time_online(0), "0s");
  }

  #[test]
  fn test_format_time_online_seconds() {
    assert_eq!(format_time_online(1), "1s");
    assert_eq!(format_time_online(30), "30s");
    assert_eq!(format_time_online(59), "59s");
  }

  #[test]
  fn test_format_time_online_minutes() {
    assert_eq!(format_time_online(60), "1m");
    assert_eq!(format_time_online(90), "1m"); // 1 minute 30 seconds -> 1m
    assert_eq!(format_time_online(120), "2m");
    assert_eq!(format_time_online(1800), "30m");
    assert_eq!(format_time_online(3599), "59m"); // 59 minutes 59 seconds -> 59m
  }

  #[test]
  fn test_format_time_online_hours() {
    assert_eq!(format_time_online(3600), "1h");
    assert_eq!(format_time_online(5400), "1h"); // 1 hour 30 minutes -> 1h
    assert_eq!(format_time_online(7200), "2h");
    assert_eq!(format_time_online(43200), "12h");
    assert_eq!(format_time_online(86399), "23h"); // 23 hours 59 minutes -> 23h
  }

  #[test]
  fn test_format_time_online_days() {
    assert_eq!(format_time_online(86400), "1d");
    assert_eq!(format_time_online(129600), "1d"); // 1 day 12 hours -> 1d
    assert_eq!(format_time_online(172800), "2d");
    assert_eq!(format_time_online(604800), "7d");
    assert_eq!(format_time_online(2592000), "30d");
  }

  #[test]
  fn test_format_time_online_boundary_cases() {
    // Test exact boundaries
    assert_eq!(format_time_online(59), "59s"); // Just before 1 minute
    assert_eq!(format_time_online(60), "1m"); // Exactly 1 minute
    assert_eq!(format_time_online(3599), "59m"); // Just before 1 hour
    assert_eq!(format_time_online(3600), "1h"); // Exactly 1 hour
    assert_eq!(format_time_online(86399), "23h"); // Just before 1 day
    assert_eq!(format_time_online(86400), "1d"); // Exactly 1 day
  }
}
