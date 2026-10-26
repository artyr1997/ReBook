//! Comic pages: images inside a CBZ (zip), or a folder of images that a CBR (rar)
//! was unpacked into on import. Page sizes come from the image headers, so the
//! reader can decide between a two-page spread and a full-width page up front.

use serde::Serialize;
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

const IMAGE_EXTS: [&str; 8] = ["jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "jxl"];

pub fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // macOS zips often carry "__MACOSX/._page.jpg" resource-fork junk
    if lower.starts_with("__macosx/") || lower.rsplit('/').next().is_some_and(|f| f.starts_with('.')) {
        return false;
    }
    lower.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTS.contains(&ext))
}

pub fn mime(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("jxl") => "image/jxl",
        _ => "image/jpeg",
    }
}

/// "page2" before "page10", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(ch) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(ch);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt)).then_with(|| m.len().cmp(&n.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
}

/// Where a comic's pages live.
#[derive(Clone, Debug)]
pub enum Source {
    /// a .cbz file
    Zip(PathBuf),
    /// a folder of images (an unpacked .cbr)
    Folder(PathBuf),
}

/// The ordered page names of a comic, with their sizes.
#[derive(Clone, Debug)]
pub struct Pages {
    pub source: Source,
    pub names: Vec<String>,
    pub sizes: Vec<PageSize>,
}

fn size_of(bytes: &[u8]) -> PageSize {
    imagesize::blob_size(bytes)
        .map(|s| PageSize { width: s.width as u32, height: s.height as u32 })
        // unreadable header: assume an ordinary portrait page
        .unwrap_or(PageSize { width: 2, height: 3 })
}

/// Reads the page list and every page's size (from the first bytes of each image).
pub fn load(source: Source) -> Result<Pages> {
    match &source {
        Source::Zip(path) => {
            let mut zip = zip::ZipArchive::new(File::open(path).map_err(err)?).map_err(err)?;
            let mut names: Vec<String> = (0..zip.len())
                .filter_map(|i| {
                    let file = zip.by_index(i).ok()?;
                    let name = file.name().ok()?.into_owned();
                    (file.is_file() && is_image(&name)).then_some(name)
                })
                .collect();
            names.sort_by(|a, b| natural_cmp(a, b));
            let mut sizes = Vec::with_capacity(names.len());
            for name in &names {
                let mut head = Vec::new();
                let mut file = zip.by_name(name).map_err(err)?;
                // headers sit at the start, but JPEG EXIF can push the size marker back
                (&mut file).take(256 * 1024).read_to_end(&mut head).map_err(err)?;
                sizes.push(size_of(&head));
            }
            Ok(Pages { source, names, sizes })
        }
        Source::Folder(dir) => {
            let mut names: Vec<String> = fs::read_dir(dir)
                .map_err(err)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_image(n))
                .collect();
            names.sort_by(|a, b| natural_cmp(a, b));
            let sizes = names
                .iter()
                .map(|n| imagesize::size(dir.join(n)).map(|s| PageSize { width: s.width as u32, height: s.height as u32 }))
                .map(|r| r.unwrap_or(PageSize { width: 2, height: 3 }))
                .collect();
            Ok(Pages { source, names, sizes })
        }
    }
}

impl Pages {
    /// The image bytes of page `index` and their MIME type.
    pub fn read(&self, index: usize) -> Result<(Vec<u8>, &'static str)> {
        let name = self.names.get(index).ok_or("No such page")?;
        let bytes = match &self.source {
            Source::Zip(path) => {
                let mut zip = zip::ZipArchive::new(File::open(path).map_err(err)?).map_err(err)?;
                let mut file = zip.by_name(name).map_err(err)?;
                let mut bytes = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut bytes).map_err(err)?;
                bytes
            }
            Source::Folder(dir) => fs::read(dir.join(name)).map_err(err)?,
        };
        Ok((bytes, mime(name)))
    }
}

/// Unpacks the images of a .cbr into `dir` (flat, names keep their order).
pub fn unpack_rar(rar: &Path, dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).map_err(err)?;
    let mut archive = unrar::Archive::new(rar).open_for_processing().map_err(err)?;
    let mut count = 0;
    while let Some(header) = archive.read_header().map_err(err)? {
        let name = header.entry().filename.to_string_lossy().replace('\\', "/");
        archive = if header.entry().is_file() && is_image(&name) {
            let (bytes, rest) = header.read().map_err(err)?;
            // flatten "chapter 1/003.jpg" → "chapter 1__003.jpg": order is kept, no path tricks
            fs::write(dir.join(name.replace('/', "__")), bytes).map_err(err)?;
            count += 1;
            rest
        } else {
            header.skip().map_err(err)?
        };
    }
    Ok(count)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// A tiny fake PNG: just the header imagesize needs.
    pub fn png(width: u32, height: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    pub fn make_cbz(path: &Path, pages: &[(&str, u32, u32)]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, w, h) in pages {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(&png(*w, *h)).unwrap();
        }
        zip.start_file("__MACOSX/._p1.png", opts).unwrap();
        zip.write_all(b"junk").unwrap();
        zip.start_file("info.txt", opts).unwrap();
        zip.write_all(b"not a page").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn zip_pages_are_ordered_sized_and_readable() {
        let dir = std::env::temp_dir().join(format!("rebook-comic-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cbz = dir.join("c.cbz");
        make_cbz(&cbz, &[("p10.png", 1600, 1000), ("p2.png", 800, 1200), ("p1.png", 800, 1200)]);

        let pages = load(Source::Zip(cbz)).unwrap();
        assert_eq!(pages.names, vec!["p1.png", "p2.png", "p10.png"]);
        assert_eq!(pages.sizes[2], PageSize { width: 1600, height: 1000 });
        let (bytes, mime) = pages.read(0).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, png(800, 1200));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn natural_page_order() {
        let mut v = vec!["10.jpg", "2.jpg", "1.jpg", "a/01.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["1.jpg", "2.jpg", "10.jpg", "a/01.jpg"]);
    }
}
