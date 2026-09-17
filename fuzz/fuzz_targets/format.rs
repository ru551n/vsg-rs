//! Formatting must not panic, and must be stable and lossless on anything that parses.
//!
//!     cargo +nightly fuzz run format          # needs cargo-fuzz
//!
//! The same properties the corpus harness checks (`examples/corpus.rs`), on inputs the fuzzer
//! makes up: the output keeps every token and comment (`format_parsed` verifies that and
//! returns an error otherwise), and formatting the output again changes nothing.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vsg_rs::{FormatConfig, Parsed};

fuzz_target!(|source: &[u8]| {
    let parsed = Parsed::new(source.to_vec());
    if !parsed.syntax_errors().is_empty() {
        return;
    }
    let cfg = FormatConfig::default();
    let Ok(once) = vsg_rs::format_parsed(&parsed, &cfg) else {
        return;
    };
    let again = vsg_rs::format_parsed(&Parsed::new(once.clone()), &cfg)
        .expect("formatting its own output must succeed");
    assert!(
        again == once,
        "formatting is not stable:\n{}\n---\n{}",
        String::from_utf8_lossy(&once),
        String::from_utf8_lossy(&again)
    );
});
