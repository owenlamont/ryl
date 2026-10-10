pub fn encoded(text: &str, width: usize, little: bool, bom: bool) -> Vec<u8> {
    let text = if bom {
        format!("\u{feff}{text}")
    } else {
        text.to_owned()
    };
    match width {
        1 => text.into_bytes(),
        2 => text
            .encode_utf16()
            .flat_map(|unit| {
                if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            })
            .collect(),
        4 => text
            .chars()
            .flat_map(|ch| {
                let unit = u32::from(ch);
                if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            })
            .collect(),
        _ => panic!("unsupported fixture width"),
    }
}
