include!(concat!(env!("OUT_DIR"), "/icons_generated.rs"));

pub fn get_pixmaps(level: u8, is_charging: bool) -> Vec<ksni::Icon> {
    vec![
        embedded_to_ksni(get_icon(level, is_charging, 16)),
        embedded_to_ksni(get_icon(level, is_charging, 22)),
        embedded_to_ksni(get_icon(level, is_charging, 32)),
        embedded_to_ksni(get_icon(level, is_charging, 48)),
    ]
}

pub fn get_placeholder_pixmaps() -> Vec<ksni::Icon> {
    vec![
        embedded_to_ksni(get_placeholder(16)),
        embedded_to_ksni(get_placeholder(22)),
        embedded_to_ksni(get_placeholder(32)),
        embedded_to_ksni(get_placeholder(48)),
    ]
}

fn embedded_to_ksni(icon: &EmbeddedIcon) -> ksni::Icon {
    ksni::Icon {
        width: icon.width,
        height: icon.height,
        data: icon.argb32.to_vec(),
    }
}
