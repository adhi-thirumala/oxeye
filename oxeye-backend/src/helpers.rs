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
