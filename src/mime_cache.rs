#[derive(Debug, PartialEq, Eq)]
pub struct MimeCache {
    header: MimeCacheHeader,
    data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct MimeCacheHeader {
    major_version: u16,
    minor_version: u16,
    alias_list_offset: u32,
    parent_list_offset: u32,
    literal_list_offset: u32,
    reverse_suffix_tree_offset: u32,
    glob_list_offset: u32,
    magic_list_offset: u32,
    namespace_list_offset: u32,
    icons_list_offset: u32,
    generic_icons_list_offset: u32,
}

pub enum Error {
    MissingHeader,
}

impl MimeCache {
    pub fn new() -> Result<Self, Error> {
        let contents = std::fs::read("/usr/share/mime/mime.cache").unwrap();
        Ok(MimeCache {
            header: MimeCacheHeader::read_header(
                contents
                    .get(0..40)
                    .ok_or(Error::MissingHeader)?
                    .try_into()
                    .expect("cant fail"),
            ),

            data: contents,
        })
    }
}

// Header:
// 2			CARD16		MAJOR_VERSION	1
// 2			CARD16		MINOR_VERSION	2
// 4			CARD32		ALIAS_LIST_OFFSET
// 4			CARD32		PARENT_LIST_OFFSET
// 4			CARD32		LITERAL_LIST_OFFSET
// 4			CARD32		REVERSE_SUFFIX_TREE_OFFSET
// 4			CARD32		GLOB_LIST_OFFSET
// 4			CARD32		MAGIC_LIST_OFFSET
// 4			CARD32		NAMESPACE_LIST_OFFSET
// 4			CARD32		ICONS_LIST_OFFSET
// 4			CARD32		GENERIC_ICONS_LIST_OFFSET
// sum = 4*9 + 4 = 40
impl MimeCacheHeader {
    fn read_header(input: &[u8; 40]) -> MimeCacheHeader {
        MimeCacheHeader {
            major_version: u8x2_u16(input[0..2].try_into().unwrap()),
            minor_version: u8x2_u16(input[2..4].try_into().unwrap()),
            alias_list_offset: u8x4_u32(input[4..8].try_into().unwrap()),
            parent_list_offset: u8x4_u32(input[8..12].try_into().unwrap()),
            literal_list_offset: u8x4_u32(input[12..16].try_into().unwrap()),
            reverse_suffix_tree_offset: u8x4_u32(input[16..20].try_into().unwrap()),
            glob_list_offset: u8x4_u32(input[20..24].try_into().unwrap()),
            magic_list_offset: u8x4_u32(input[24..28].try_into().unwrap()),
            namespace_list_offset: u8x4_u32(input[28..32].try_into().unwrap()),
            icons_list_offset: u8x4_u32(input[32..36].try_into().unwrap()),
            generic_icons_list_offset: u8x4_u32(input[36..40].try_into().unwrap()),
        }
    }
}

fn u8x2_u16(input: &[u8; 2]) -> u16 {
    ((input[0] as u16) << 8) | input[1] as u16
}

fn u8x4_u32(input: &[u8; 4]) -> u32 {
    ((input[0] as u32) << 24)
        | ((input[1] as u32) << (16))
        | ((input[2] as u32) << (8))
        | input[3] as u32
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn u8x2_u16_test() {
        assert_eq!(u8x2_u16(&[0, 1]), 1);
        assert_eq!(u8x2_u16(&[0, 2]), 2);
        assert_eq!(u8x2_u16(&[1, 0]), 256);
        assert_eq!(u8x2_u16(&[1, 1]), 257);
        assert_eq!(u8x2_u16(&[122, 144]), 31376);
        assert_eq!(u8x2_u16(&[255, 255]), u16::MAX);
    }

    #[test]
    fn u8x4_u32_test() {
        assert_eq!(u8x4_u32(&[0, 0, 0, 1]), 1);
        assert_eq!(u8x4_u32(&[0, 0, 1, 0]), 256);
        assert_eq!(u8x4_u32(&[0, 0, 122, 144]), 31376);
        assert_eq!(u8x4_u32(&[255, 255, 255, 255]), u32::MAX);
    }

    #[test]
    fn mime_cache() {
        assert!(MimeCache::new().is_ok())
    }
}
