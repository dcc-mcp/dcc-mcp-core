use dcc_mcp_jsonrpc::{decode_cursor, encode_cursor};

#[test]
fn unicode_cursors_are_rejected_without_panicking() {
    // Even byte lengths can still split UTF-8 characters at a hex-pair boundary.
    for cursor in ["0é0", "é", "🦀", "0€", "0🦀0", "\u{200b}0", "中0"] {
        assert_eq!(decode_cursor(cursor), None, "cursor: {cursor:?}");
    }
}

#[test]
fn generated_cursors_keep_their_wire_format_and_round_trip() {
    assert_eq!(encode_cursor(0), "30");
    assert_eq!(encode_cursor(64), "3634");

    for offset in [0, 1, 31, 32, 63, 64, usize::MAX - 1, usize::MAX] {
        assert_eq!(decode_cursor(&encode_cursor(offset)), Some(offset));
    }
}

#[test]
fn malformed_hex_and_non_offsets_are_rejected() {
    for cursor in ["", "3", "gg", "3z", "ff", "2d31", "312e30", "2031", "31\n"] {
        assert_eq!(decode_cursor(cursor), None, "cursor: {cursor:?}");
    }
}

#[test]
fn numeric_overflow_is_rejected() {
    let decimal_digits = usize::MAX.to_string().len();
    // The largest decimal integer of this width exceeds usize::MAX.
    assert_eq!(decode_cursor(&"39".repeat(decimal_digits)), None);
}

#[test]
fn cursors_longer_than_any_generated_token_are_rejected() {
    let decimal_digits = usize::MAX.to_string().len();
    // Leading zeros would otherwise parse successfully despite unbounded input.
    assert_eq!(decode_cursor(&"30".repeat(decimal_digits + 1)), None);
    assert_eq!(decode_cursor(&"30".repeat(4096)), None);
}

#[test]
fn bounded_legacy_numeric_spellings_remain_accepted() {
    assert_eq!(decode_cursor("303031"), Some(1));
    assert_eq!(decode_cursor("2b31"), Some(1));
    assert_eq!(decode_cursor("2B31"), Some(1));
}
