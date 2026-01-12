use std::{collections::HashMap, ffi::CStr};

#[derive(Debug, PartialEq, Eq)]
pub struct MimeType(pub String);

impl From<String> for MimeType {
    fn from(value: String) -> Self {
        MimeType(value)
    }
}

#[derive(Debug)]
pub struct MimeSearcher {
    mime_cache: MimeCache,
    globber: Globber,
}

#[derive(Debug)]
struct MimeCache {
    cache_header: MimeCacheHeader,
    cache_data: Vec<u8>,
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

#[derive(Debug)]
struct Globber {
    globs2_data: String,
    simple_globing_map: HashMap<String, GlobEntry>,
}

#[derive(Debug)]
struct GlobEntry {
    weight: u8,
    mime: MimeType,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    MimeCacheNotFound,
    Globs2NotFound,
    Globs2BadLine(String),
    NotANumber,
    MissingHeader,
    MissingGenericIconsList,
    NoIconFound,
    CstrUnterminated,
    InvalidUTF8,
}

impl MimeCache {
    fn new() -> Result<Self, Error> {
        let cache_contents =
            std::fs::read("/usr/share/mime/mime.cache").map_err(|_| Error::MimeCacheNotFound)?;
        Ok(MimeCache {
            cache_header: MimeCacheHeader::read_header(
                cache_contents
                    .get(0..40)
                    .ok_or(Error::MissingHeader)?
                    .try_into()
                    .expect("cant fail"),
            ),
            cache_data: cache_contents,
        })
    }

    // GenericIconsList:
    // IconsList:
    // 4			CARD32		N_ICONS
    // 8*N_ICONS	IconListEntry
    //
    // IconListEntry:
    // 4			CARD32		MIME_TYPE_OFFSET
    // 4			CARD32		ICON_NAME_OFFSET
    fn find_icon_for_mimetype(&self, mime_type: &str) -> Result<MimeType, Error> {
        const STRIDE: usize = 8;

        let start = self.cache_header.generic_icons_list_offset as usize;

        let num_icons = get_u32_panics(self.cache_data.as_slice(), start);

        let list_start = start + 4;

        for i in (list_start..list_start + num_icons as usize * STRIDE).step_by(STRIDE) {
            let mime_type_offset = get_u32_panics(self.cache_data.as_slice(), i) as usize;
            let found_mime_type =
                CStr::from_bytes_until_nul(self.cache_data.get(mime_type_offset..).unwrap())
                    .map_err(|_e| Error::CstrUnterminated)?
                    .to_str()
                    .map_err(|_| Error::InvalidUTF8)?;

            if found_mime_type == mime_type {
                // Only load icon name if we have matched
                let icon_name_offset = get_u32_panics(self.cache_data.as_slice(), i + 4) as usize;
                let icon_name =
                    CStr::from_bytes_until_nul(self.cache_data.get(icon_name_offset..).unwrap())
                        .map_err(|_e| Error::CstrUnterminated)?
                        .to_str()
                        .map_err(|_| Error::InvalidUTF8)?;

                return Ok(icon_name.to_string().into());
            }
        }

        Err(Error::NoIconFound)
    }
}

impl Globber {
    fn new(cache: &MimeCache) -> Result<Self, Error> {
        let mut hashmap = HashMap::new();
        for (k, v) in Self::get_globs_from_cache(cache)?
            .into_iter()
            .filter(|(k, _)| !k.contains('?') && !k.contains('['))
        {
            let Some(k) = k.strip_prefix(".*") else {
                continue;
            };

            hashmap.insert(k.to_string(), v);
        }

        let globs2_data =
            std::fs::read_to_string("/usr/share/mime/globs2").map_err(|_| Error::Globs2NotFound)?;

        for (k, v) in Self::get_globs2_data(&globs2_data)?
            .into_iter()
            .filter(|(k, _)| !k.contains('?') && !k.contains('['))
        {
            let Some(k) = k.strip_prefix(".*") else {
                continue;
            };

            hashmap.insert(k.to_string(), v);
        }
        println!("glob hashmap: {:#?}", hashmap);

        Ok(Globber {
            globs2_data,

            simple_globing_map: hashmap,
        })
    }

    // GlobList:
    // 4			CARD32		N_GLOBS
    // 12*N_GLOBS	GlobEntry
    //
    // GlobEntry:
    //
    // 4			CARD32		GLOB_OFFSET
    // 4			CARD32		MIME_TYPE_OFFSET
    // 4			CARD32		WEIGHT in lower 8 bits
    //                              FLAGS in rest:
    //                              0x100 = case-sensitive
    fn get_globs_from_cache(cache: &MimeCache) -> Result<Vec<(String, GlobEntry)>, Error> {
        const STRIDE: usize = 12;

        let start = cache.cache_header.glob_list_offset as usize;

        let num_globs = get_u32_panics(cache.cache_data.as_slice(), start);

        let list_start = start + 4;

        let mut output = Vec::new();

        for i in (list_start..list_start + num_globs as usize * STRIDE).step_by(STRIDE) {
            let glob_offset = get_u32_panics(cache.cache_data.as_slice(), i) as usize;

            let glob = CStr::from_bytes_until_nul(cache.cache_data.get(glob_offset..).unwrap())
                .map_err(|_| Error::CstrUnterminated)?
                .to_str()
                .map_err(|_| Error::InvalidUTF8)?;

            let mime_offset = get_u32_panics(cache.cache_data.as_slice(), i + 4) as usize;

            let mime = CStr::from_bytes_until_nul(cache.cache_data.get(mime_offset..).unwrap())
                .map_err(|_| Error::CstrUnterminated)?
                .to_str()
                .map_err(|_| Error::InvalidUTF8)?;

            let meta = get_u32_panics(cache.cache_data.as_slice(), i + 8) as usize;

            let weight = (meta & 0xFF) as u8;

            output.push((
                glob.to_string(),
                GlobEntry {
                    mime: mime.to_string().into(),
                    weight,
                },
            ));
        }
        Ok(output)
    }

    fn get_globs2_data(globs: &str) -> Result<Vec<(String, GlobEntry)>, Error> {
        let mut output = Vec::new();
        for line in globs.lines() {
            if line.starts_with('#') {
                continue;
            }
            let line_conents: Vec<&str> = line.splitn(3, ':').collect();
            if line_conents.len() != 3 {
                return Err(Error::Globs2BadLine(line.to_string()));
            }

            let (weight_raw, mime_string, glob_string) = (
                line_conents[0].to_string(),
                line_conents[1].to_string(),
                line_conents[2].to_string(),
            );

            output.push((
                glob_string,
                GlobEntry {
                    weight: weight_raw.parse().map_err(|_| Error::NotANumber)?,
                    mime: mime_string.into(),
                },
            ));
        }
        Ok(output)
    }
}

impl MimeSearcher {
    pub fn new() -> Result<Self, Error> {
        let mime_cache = MimeCache::new()?;
        Ok(MimeSearcher {
            globber: Globber::new(&mime_cache)?,
            mime_cache,
        })
    }

    pub fn find_icon_for_mimetype(&self, mime_type: &str) -> Result<MimeType, Error> {
        self.mime_cache.find_icon_for_mimetype(mime_type)
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

// fn get_u16_panics(data: &[u8], index: usize) -> u16 {
//     u8x2_u16(data[index..index + 2].try_into().unwrap())
// }

fn u8x4_u32(input: &[u8; 4]) -> u32 {
    ((input[0] as u32) << 24)
        | ((input[1] as u32) << (16))
        | ((input[2] as u32) << (8))
        | input[3] as u32
}

/// Panics all the time
fn get_u32_panics(data: &[u8], index: usize) -> u32 {
    u8x4_u32(data[index..index + 4].try_into().unwrap())
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
        // let searcher = MimeSearcher::new();
    }

    #[test]
    fn get_icon_for_mimetype() {
        let cache = MimeCache::new().unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            cache.find_icon_for_mimetype("font/otf"),
            Ok("font-x-generic".to_string().into())
        );
        assert_eq!(
            cache.find_icon_for_mimetype("text/javascript"),
            Ok("text-x-script".to_string().into())
        );
        assert_eq!(
            cache.find_icon_for_mimetype("application/pdf"),
            Ok("x-office-document".to_string().into())
        );
        println!("Time to find icon: {:#?}", start.elapsed());
    }

    #[test]
    fn get_mimetype_for_filename() {
        let cache = Globber::new(&MimeCache::new().unwrap()).unwrap();
        let start = std::time::Instant::now();
        // assert_eq!(
        //     cache.glob_filename_to_mimetype("foo.pdf"),
        //     Ok("font-x-generic".to_string())
        // );
        println!("Time to find icon: {:#?}", start.elapsed());
    }
}
