//! Embedded Bookerly font assets for e-reader deployments.

/// Embedded font binary with display metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedFont {
    pub family: &'static str,
    pub style: &'static str,
    pub data: &'static [u8],
}

pub const BOOKERLY_REGULAR: EmbeddedFont = EmbeddedFont {
    family: "Bookerly",
    style: "Regular",
    data: include_bytes!("../assets/fonts/bookerly/Bookerly-Regular.ttf"),
};

pub const BOOKERLY_BOLD: EmbeddedFont = EmbeddedFont {
    family: "Bookerly",
    style: "Bold",
    data: include_bytes!("../assets/fonts/bookerly/Bookerly-Bold.ttf"),
};

pub const BOOKERLY_ITALIC: EmbeddedFont = EmbeddedFont {
    family: "Bookerly",
    style: "Italic",
    data: include_bytes!("../assets/fonts/bookerly/Bookerly-Italic.ttf"),
};

pub const BOOKERLY_BOLD_ITALIC: EmbeddedFont = EmbeddedFont {
    family: "Bookerly",
    style: "BoldItalic",
    data: include_bytes!("../assets/fonts/bookerly/Bookerly-BoldItalic.ttf"),
};

pub const BOOKERLY_SET: [EmbeddedFont; 4] = [
    BOOKERLY_REGULAR,
    BOOKERLY_BOLD,
    BOOKERLY_ITALIC,
    BOOKERLY_BOLD_ITALIC,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_fonts_are_present() {
        for font in BOOKERLY_SET {
            assert!(!font.data.is_empty());
        }
    }
}
