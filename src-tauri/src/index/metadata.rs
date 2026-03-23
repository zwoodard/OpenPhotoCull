use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifMetadata {
    pub date_time_original: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub aperture: Option<f64>,
    pub iso: Option<u32>,
    pub shutter_speed: Option<String>,
    /// EXIF orientation tag (1-8). 1 = normal, others require rotation/flip.
    pub orientation: Option<u16>,
}

pub fn extract_metadata(path: &Path) -> Option<ExifMetadata> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;

    let date_time_original = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    let gps_lat = extract_gps_coord(&exif, exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef);
    let gps_lng = extract_gps_coord(&exif, exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef);

    let camera_make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string().trim_matches('"').to_string());

    let camera_model = exif
        .get_field(exif::Tag::Model, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string().trim_matches('"').to_string());

    let focal_length_mm = exif
        .get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
        .and_then(|f| parse_rational_f64(&f.value));

    let aperture = exif
        .get_field(exif::Tag::FNumber, exif::In::PRIMARY)
        .and_then(|f| parse_rational_f64(&f.value));

    let iso = exif
        .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Short(v) => v.first().map(|&x| x as u32),
            exif::Value::Long(v) => v.first().copied(),
            _ => None,
        });

    let shutter_speed = exif
        .get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Short(v) => v.first().copied(),
            _ => None,
        });

    Some(ExifMetadata {
        date_time_original,
        gps_lat,
        gps_lng,
        camera_make,
        camera_model,
        focal_length_mm,
        aperture,
        iso,
        shutter_speed,
        orientation,
    })
}

/// Read just the EXIF orientation tag from a file. Fast (header only).
pub fn read_orientation(path: &Path) -> Option<u16> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Short(v) => v.first().copied(),
            _ => None,
        })
}

/// Apply EXIF orientation transform to a DynamicImage.
/// Orientation values:
///   1 = normal
///   2 = flip horizontal
///   3 = rotate 180
///   4 = flip vertical
///   5 = transpose (rotate 90 CW + flip horizontal)
///   6 = rotate 90 CW
///   7 = transverse (rotate 90 CCW + flip horizontal)
///   8 = rotate 90 CCW
pub fn apply_orientation(img: image::DynamicImage, orientation: u16) -> image::DynamicImage {
    use image::imageops;
    match orientation {
        1 => img, // Normal
        2 => DynamicImage::ImageRgba8(imageops::flip_horizontal(&img)),
        3 => DynamicImage::ImageRgba8(imageops::rotate180(&img)),
        4 => DynamicImage::ImageRgba8(imageops::flip_vertical(&img)),
        5 => {
            let rotated = imageops::rotate90(&img);
            DynamicImage::ImageRgba8(imageops::flip_horizontal(&DynamicImage::ImageRgba8(rotated)))
        }
        6 => DynamicImage::ImageRgba8(imageops::rotate90(&img)),
        7 => {
            let rotated = imageops::rotate270(&img);
            DynamicImage::ImageRgba8(imageops::flip_horizontal(&DynamicImage::ImageRgba8(rotated)))
        }
        8 => DynamicImage::ImageRgba8(imageops::rotate270(&img)),
        _ => img, // Unknown, leave as-is
    }
}

use image::DynamicImage;

fn extract_gps_coord(
    exif: &exif::Exif,
    coord_tag: exif::Tag,
    ref_tag: exif::Tag,
) -> Option<f64> {
    let field = exif.get_field(coord_tag, exif::In::PRIMARY)?;
    let rationals = match &field.value {
        exif::Value::Rational(v) if v.len() >= 3 => v,
        _ => return None,
    };

    let degrees = rationals[0].to_f64();
    let minutes = rationals[1].to_f64();
    let seconds = rationals[2].to_f64();
    let mut coord = degrees + minutes / 60.0 + seconds / 3600.0;

    if let Some(ref_field) = exif.get_field(ref_tag, exif::In::PRIMARY) {
        let ref_str = ref_field.display_value().to_string();
        if ref_str.contains('S') || ref_str.contains('W') {
            coord = -coord;
        }
    }

    Some(coord)
}

fn parse_rational_f64(value: &exif::Value) -> Option<f64> {
    match value {
        exif::Value::Rational(v) => v.first().map(|r| r.to_f64()),
        _ => None,
    }
}

/// Parse EXIF date strings like "2024:01:15 14:30:00" to Unix timestamp.
pub fn parse_exif_date(date_str: &str) -> Option<i64> {
    let cleaned = date_str.replace('"', "").trim().to_string();
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y:%m:%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    None
}
