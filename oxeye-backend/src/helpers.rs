use rand::distr::{Alphanumeric, SampleString};
use rand::rng;

fn generate_code() -> String {
    format!("oxeye-{}", Alphanumeric.sample_string(&mut rng(), 6))
}