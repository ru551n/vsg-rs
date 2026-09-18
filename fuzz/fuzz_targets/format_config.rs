//! Formatting must be stable under any configuration, not only the default one.
//!
//!     cargo +nightly fuzz run format_config
//!
//! The first bytes of the input choose the settings that change the layout the most (width,
//! indentation, keyword case, alignment, blank lines, port mode spacing, continuation indent);
//! the rest is the VHDL. Formatting twice must give the same bytes, and the output must keep
//! every token and comment (`format_parsed` checks that and returns an error otherwise).

#![no_main]

use libfuzzer_sys::fuzz_target;
use vsg_rs::{Config, Parsed};

fn pick<'a>(byte: u8, options: &[&'a str]) -> &'a str {
    options[byte as usize % options.len()]
}

fn configuration(knobs: &[u8; 4]) -> String {
    let mut yaml = String::from("rule:\n  global:\n");
    yaml += &format!("    case: {}\n", pick(knobs[0], &["lower", "upper"]));
    yaml += &format!("    indent_size: {}\n", pick(knobs[0] >> 1, &["2", "3", "4"]));
    yaml += &format!(
        "    indent_style: {}\n",
        pick(knobs[1], &["spaces", "smart_tabs"])
    );
    yaml += &format!(
        "  length_001:\n    length: {}\n",
        pick(knobs[1] >> 2, &["40", "80", "120", "200"])
    );
    let mut groups = String::new();
    if knobs[2] & 1 == 0 {
        groups += "    alignment:\n      disable: true\n";
    }
    if knobs[2] & 2 == 0 {
        groups += "    blank_line:\n      disable: true\n";
    }
    if !groups.is_empty() {
        yaml += "  group:\n";
        yaml += &groups;
    }
    if knobs[3] & 1 == 0 {
        yaml += "  port_007:\n    spaces_after: 1\n  port_008:\n    spaces_after: 1\n";
    }
    if knobs[3] & 2 == 0 {
        yaml += "  concurrent_003:\n    align_left: 'yes'\n    align_paren: 'no'\n";
    }
    if knobs[3] & 4 == 0 {
        yaml += "  generic_010:\n    action: same_line\n  port_014:\n    action: same_line\n";
    }
    yaml
}

fuzz_target!(|data: &[u8]| {
    let Some((knobs, source)) = data.split_first_chunk::<4>() else {
        return;
    };
    let config = Config::parse(&configuration(knobs)).expect("generated configuration is valid");
    let parsed = Parsed::new(source.to_vec());
    if !parsed.syntax_errors().is_empty() {
        return;
    }
    let Ok(once) = vsg_rs::format_parsed(&parsed, &config.format) else {
        return;
    };
    let again = vsg_rs::format_parsed(&Parsed::new(once.clone()), &config.format)
        .expect("formatting its own output must succeed");
    assert!(
        again == once,
        "formatting is not stable under:\n{}\n---\n{}\n---\n{}",
        configuration(knobs),
        String::from_utf8_lossy(&once),
        String::from_utf8_lossy(&again)
    );
});
