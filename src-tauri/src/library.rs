//! The user's own photos, uploaded from the control panel.
//!
//! Every upload is decoded, turned upright (phones store rotation as metadata), scaled
//! down to screen size and re-encoded as JPEG. That keeps a 12 MP phone photo from
//! costing a Pi 50 MB to decode, gives the panel a small thumbnail to show, and drops
//! all metadata - GPS included - since the files are reachable over the network.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::settings_manager;

pub const MAX_PHOTOS: usize = 200;
/// Raw upload size. Phone photos are 3-12 MB; this leaves room for a big PNG.
pub const MAX_UPLOAD_BYTES: usize = 30 * 1024 * 1024;
/// Longest edge kept. Sharp on a 1440p screen, still light for a Pi.
const MAX_EDGE: u32 = 2560;
const THUMB_EDGE: u32 = 400;
const QUALITY: u8 = 85;

pub fn dir() -> Result<PathBuf, String> {
    Ok(settings_manager::get_settings_path()?.with_file_name("photos"))
}

/// Ids are 16 lowercase hex characters that we generated. Checking the shape before
/// any file access is what keeps `../settings` out of a path.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn photo_path(id: &str) -> Option<PathBuf> {
    photo_path_in(&dir().ok()?, id)
}

pub fn thumb_path(id: &str) -> Option<PathBuf> {
    thumb_path_in(&dir().ok()?, id)
}

pub fn list() -> Vec<String> {
    dir().map(|dir| list_in(&dir)).unwrap_or_default()
}

pub fn add(bytes: &[u8]) -> Result<String, AddError> {
    add_in(&dir().map_err(AddError::Storage)?, bytes)
}

pub fn remove(id: &str) -> Result<bool, String> {
    remove_in(&dir()?, id)
}

/// A random photo other than `current`, if there is another to pick.
pub fn pick(current: Option<&str>) -> Option<String> {
    use rand::seq::SliceRandom;
    let ids = list();
    let others: Vec<&String> = ids.iter().filter(|id| Some(id.as_str()) != current).collect();
    others
        .choose(&mut rand::thread_rng())
        .map(|id| id.to_string())
        .or_else(|| ids.first().cloned())
}

#[derive(Debug, PartialEq)]
pub enum AddError {
    NotAnImage,
    Full,
    Storage(String),
}

fn photo_path_in(dir: &Path, id: &str) -> Option<PathBuf> {
    is_valid_id(id).then(|| dir.join(format!("{id}.jpg")))
}

fn thumb_path_in(dir: &Path, id: &str) -> Option<PathBuf> {
    is_valid_id(id).then(|| dir.join("thumbs").join(format!("{id}.jpg")))
}

fn list_in(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".jpg")?.to_string();
            is_valid_id(&id).then_some(id)
        })
        .collect();
    ids.sort();
    ids
}

// ponytail: the count check and the write are not atomic, so two simultaneous uploads
// can land one over MAX_PHOTOS. Harmless; lock around it if the limit ever matters.
fn add_in(dir: &Path, bytes: &[u8]) -> Result<String, AddError> {
    if list_in(dir).len() >= MAX_PHOTOS {
        return Err(AddError::Full);
    }

    let photo = decode_upright(bytes).ok_or(AddError::NotAnImage)?;
    let photo = if photo.width().max(photo.height()) > MAX_EDGE {
        photo.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
    } else {
        photo
    };
    let thumb = photo.thumbnail(THUMB_EDGE, THUMB_EDGE);

    let id = format!("{:016x}", rand::random::<u64>());
    let storage = |e: String| AddError::Storage(e);
    std::fs::create_dir_all(dir.join("thumbs")).map_err(|e| storage(e.to_string()))?;

    let thumb_file = thumb_path_in(dir, &id).expect("generated ids are valid");
    let photo_file = photo_path_in(dir, &id).expect("generated ids are valid");
    write_jpeg(&thumb, &thumb_file).map_err(storage)?;
    if let Err(e) = write_jpeg(&photo, &photo_file) {
        let _ = std::fs::remove_file(&thumb_file);
        return Err(storage(e));
    }
    Ok(id)
}

fn remove_in(dir: &Path, id: &str) -> Result<bool, String> {
    let (Some(photo), Some(thumb)) = (photo_path_in(dir, id), thumb_path_in(dir, id)) else {
        return Ok(false);
    };
    if !photo.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&photo).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(thumb);
    Ok(true)
}

/// Decode only the formats compiled in (JPEG, PNG, WebP) and apply EXIF rotation.
fn decode_upright(bytes: &[u8]) -> Option<DynamicImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut photo = DynamicImage::from_decoder(decoder).ok()?;
    photo.apply_orientation(orientation);
    Some(photo)
}

fn write_jpeg(photo: &DynamicImage, path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), QUALITY);
    photo.to_rgb8().write_with_encoder(encoder).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("idleview-library-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::new_rgb8(width, height)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn an_upload_is_stored_downscaled_with_a_thumbnail() {
        let dir = temp_dir();
        let id = add_in(&dir, &png(4000, 1000)).unwrap();

        assert!(is_valid_id(&id));
        assert_eq!(list_in(&dir), vec![id.clone()]);

        let stored = image::open(photo_path_in(&dir, &id).unwrap()).unwrap();
        assert_eq!((stored.width(), stored.height()), (2560, 640));
        let thumb = image::open(thumb_path_in(&dir, &id).unwrap()).unwrap();
        assert!(thumb.width() <= THUMB_EDGE && thumb.height() <= THUMB_EDGE);

        assert_eq!(remove_in(&dir, &id), Ok(true));
        assert!(list_in(&dir).is_empty());
        assert!(!thumb_path_in(&dir, &id).unwrap().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn anything_but_an_image_is_refused() {
        let dir = temp_dir();
        assert_eq!(add_in(&dir, b"not an image"), Err(AddError::NotAnImage));
        assert_eq!(add_in(&dir, b"<svg xmlns='http://www.w3.org/2000/svg'/>"), Err(AddError::NotAnImage));
        assert!(list_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ids_cannot_reach_outside_the_library() {
        let dir = Path::new("/library");
        for id in ["../settings", "..\\settings", "0123456789abcdeg", "0123456789ABCDEF", "", "abc"] {
            assert!(photo_path_in(dir, id).is_none(), "{id:?} should be refused");
            assert_eq!(remove_in(dir, id), Ok(false));
        }
        assert!(photo_path_in(dir, "0123456789abcdef").is_some());
    }
}
