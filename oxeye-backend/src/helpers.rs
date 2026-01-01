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
    format!("{}s", duration_secs)
  } else if duration_secs < HOUR {
    let minutes = duration_secs / MINUTE;
    format!("{}m", minutes)
  } else if duration_secs < DAY {
    let hours = duration_secs / HOUR;
    format!("{}h", hours)
  } else {
    let days = duration_secs / DAY;
    format!("{}d", days)
  }
}
