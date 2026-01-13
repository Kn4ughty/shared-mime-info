//! An implementation of the [shared mime info spec](https://www.freedesktop.org/wiki/Software/shared-mime-info/)
//!
//! See `MimeSearcher` functions for full list of operations.
//!
//! # Example
//! Find icon name for file from filename:
//!
//! ```
//! use shared_mime_info as smi;
//!
//! let searcher = smi::MimeSearcher::new().unwrap();
//!
//! let mime_type =
//! searcher.find_mimetype_from_filepath(&std::path::PathBuf::from("foo.pdf")).unwrap();
//! let icon_name = searcher.find_icon_for_mimetype(mime_type).unwrap();
//! ```
//!

// https://specifications.freedesktop.org/shared-mime-info/0.21/ar01s02.html

use std::{cmp::Ordering, collections::HashMap, ffi::CStr, path::Path};

/// String wrapper. Used to make typing clearer
#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct MimeType(pub String);

impl From<String> for MimeType {
    fn from(value: String) -> Self {
        MimeType(value)
    }
}

/// The mime type searcher, loads all data from file system when created.
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
    fn find_icon_for_mimetype(&self, mime_type: MimeType) -> Result<String, Error> {
        // Takes in a mimetype, e.g:
        // application/pdf -> x-office-document

        const STRIDE: usize = 8;

        let start = self.cache_header.generic_icons_list_offset as usize;

        let num_icons = get_u32_panics(self.cache_data.as_slice(), start);

        let list_start = start + 4;

        // The given list is sorted, meaning a binary search can be done

        let mut min_index: usize = 0;
        let mut max_index: usize = num_icons as usize;
        let mut index: usize = max_index / 2;

        loop {
            let ptr = list_start + index * STRIDE;

            let mime_type_offset = get_u32_panics(self.cache_data.as_slice(), ptr) as usize;
            let found_mime_type: MimeType =
                CStr::from_bytes_until_nul(self.cache_data.get(mime_type_offset..).unwrap())
                    .map_err(|_e| Error::CstrUnterminated)?
                    .to_str()
                    .map_err(|_| Error::InvalidUTF8)?
                    .to_string()
                    .into();

            let ord = mime_type.cmp(&found_mime_type);
            if ord == Ordering::Less {
                max_index = index;
                index = (max_index + min_index) / 2;
            } else if ord == Ordering::Greater {
                min_index = index;
                index = (max_index + min_index) / 2;
            } else {
                debug_assert_eq!(found_mime_type, mime_type);
                // Only load icon name if we have matched
                let icon_name_offset = get_u32_panics(self.cache_data.as_slice(), ptr + 4) as usize;
                let icon_name =
                    CStr::from_bytes_until_nul(self.cache_data.get(icon_name_offset..).unwrap())
                        .map_err(|_e| Error::CstrUnterminated)?
                        .to_str()
                        .map_err(|_| Error::InvalidUTF8)?;

                return Ok(icon_name.to_string());
            }

            if index == max_index || index == min_index {
                break;
            }
        }

        Err(Error::NoIconFound)
    }
}

impl Globber {
    fn new(cache: &MimeCache) -> Result<Self, Error> {
        let mut hashmap = HashMap::new();

        let globs2_data =
            std::fs::read_to_string("/usr/share/mime/globs2").map_err(|_| Error::Globs2NotFound)?;

        for (k, v) in Self::get_globs_from_cache(cache)?
            .into_iter()
            .chain(Self::get_globs2_data(&globs2_data)?.into_iter())
        {
            if let Some(k) = k.strip_prefix("*.")
                && !(k.contains('?') || k.contains('['))
            {
                hashmap.insert(k.to_string(), v);
            } else {
                // TODO. Add to a vec for complex globs
                continue;
            };
        }
        // println!("glob hashmap: {:#?}", hashmap);

        Ok(Globber {
            globs2_data,
            simple_globing_map: hashmap,
        })
    }

    fn lookup_filename(&self, name: &std::path::Path) -> Option<MimeType> {
        if let Some(ext) = name.extension()
            && let Some(entry) = self.simple_globing_map.get(ext.to_str()?)
        {
            return Some(entry.mime.clone());
        }
        None
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

    /// Finds the icon name for a mimetype. To get the actual image you would need to use a crate like
    /// [`icon`](https://crates.io/crates/icon)
    pub fn find_icon_for_mimetype(&self, mime_type: MimeType) -> Result<String, Error> {
        self.mime_cache.find_icon_for_mimetype(mime_type)
    }

    /// Finds the mimetype from a filepath.
    ///
    /// Looks at the content in MIME/globs2 and mime.cache.
    /// It starts with a map of just *.xxx file extensions so that `path.extension()` can be used in
    /// an internal hashmap.
    ///
    /// *This is unimplemented:*
    /// If those both fail, it can use magic (numbers).
    pub fn find_mimetype_from_filepath(&self, path: &Path) -> Option<MimeType> {
        self.globber.lookup_filename(path)
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
            major_version: u16::from_be_bytes(input[0..2].try_into().unwrap()),
            minor_version: u16::from_be_bytes(input[2..4].try_into().unwrap()),
            alias_list_offset: u32::from_be_bytes(input[4..8].try_into().unwrap()),
            parent_list_offset: u32::from_be_bytes(input[8..12].try_into().unwrap()),
            literal_list_offset: u32::from_be_bytes(input[12..16].try_into().unwrap()),
            reverse_suffix_tree_offset: u32::from_be_bytes(input[16..20].try_into().unwrap()),
            glob_list_offset: u32::from_be_bytes(input[20..24].try_into().unwrap()),
            magic_list_offset: u32::from_be_bytes(input[24..28].try_into().unwrap()),
            namespace_list_offset: u32::from_be_bytes(input[28..32].try_into().unwrap()),
            icons_list_offset: u32::from_be_bytes(input[32..36].try_into().unwrap()),
            generic_icons_list_offset: u32::from_be_bytes(input[36..40].try_into().unwrap()),
        }
    }
}

/// Panics all the time
fn get_u32_panics(data: &[u8], index: usize) -> u32 {
    u32::from_be_bytes(data[index..index + 4].try_into().unwrap())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn get_icon_for_mimetype() {
        let cache = MimeCache::new().unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            cache.find_icon_for_mimetype(MimeType("font/otf".to_string())),
            Ok("font-x-generic".to_string().into())
        );
        assert_eq!(
            cache.find_icon_for_mimetype(MimeType("text/javascript".to_string())),
            Ok("text-x-script".to_string().into())
        );
        assert_eq!(
            cache.find_icon_for_mimetype(MimeType("application/pdf".to_string())),
            Ok("x-office-document".to_string().into())
        );
        assert_eq!(
            cache.find_icon_for_mimetype(MimeType("not_a_real_mimetype1234".to_string())),
            Err(Error::NoIconFound)
        );
        println!("Time to find icon: {:#?}", start.elapsed());
    }

    #[test]
    fn get_mimetype_for_filename() {
        let cache = Globber::new(&MimeCache::new().unwrap()).unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            cache.lookup_filename(&std::path::PathBuf::from("foo.pdf")),
            Some("application/pdf".to_string().into())
        );
        assert_eq!(
            cache.lookup_filename(&std::path::PathBuf::from("bar.srt")),
            Some("application/x-subrip".to_string().into())
        );
        assert_eq!(
            cache.lookup_filename(&std::path::PathBuf::from("baz.md")),
            Some("text/markdown".to_string().into())
        );
        println!("Time to find mimetype: {:#?}", start.elapsed());
    }
}
