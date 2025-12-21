use rand::distr::{Alphanumeric, SampleString};
use rand::rng;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn generate_code() -> String {
  format!("oxeye-{}", Alphanumeric.sample_string(&mut rng(), 6))
}

pub(crate) fn generate_api_key() -> String {
  format!("oxeye-sk-{}", Alphanumeric.sample_string(&mut rng(), 32))
}

pub(crate) fn hash_api_key(key: &String) -> String {
  format! {"{:x}", Sha256::digest(key.as_bytes())}
}

pub(crate) fn now() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64
}
