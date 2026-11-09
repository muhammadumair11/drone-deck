use super::*;
#[test]
fn huge_unterminated_output_is_bounded_and_next_line_survives() {
    let mut input = vec![b'x'; 4_000_000];
    input.extend_from_slice(b"\n@telemetry {\"link\":\"Connected\"}\nlast");
    let lines: Vec<_> = BoundedLines::new(std::io::BufReader::new(input.as_slice()))
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(lines[0].len(), 4096);
    assert_eq!(lines[1], "@telemetry {\"link\":\"Connected\"}");
    assert_eq!(lines[2], "last");
}
