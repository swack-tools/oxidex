//! JPEG quality estimation from DQT quantization tables.
//!
//! Verbatim port of ExifTool's EstimateQuality (JPEGDigest.pm v1.06), itself
//! derived from ImageMagick coders/jpeg.c. Inputs are raw DQT segment payloads
//! (marker 0xFFDB) indexed by table id (first byte & 0x0F, ids 0-3), matching
//! how ExifTool's ProcessJPEG collects them. Each payload is walked in 65-byte
//! strides (1 precision/id byte + 64 8-bit values), up to 4 tables total.

/// Threshold tables for color images (>= 2 quantization tables), from
/// ExifTool JPEGDigest.pm / ImageMagick. Index i corresponds to quality i+1.
const COLOR_HASH: [u32; 100] = [
    1020, 1015, 932, 848, 780, 735, 702, 679, 660, 645, //
    632, 623, 613, 607, 600, 594, 589, 585, 581, 571, //
    555, 542, 529, 514, 494, 474, 457, 439, 424, 410, //
    397, 386, 373, 364, 351, 341, 334, 324, 317, 309, //
    299, 294, 287, 279, 274, 267, 262, 257, 251, 247, //
    243, 237, 232, 227, 222, 217, 213, 207, 202, 198, //
    192, 188, 183, 177, 173, 168, 163, 157, 153, 148, //
    143, 139, 132, 128, 125, 119, 115, 108, 104, 99, //
    94, 90, 84, 79, 74, 70, 64, 59, 55, 49, //
    45, 40, 34, 30, 25, 20, 15, 11, 6, 4,
];

const COLOR_SUMS: [u32; 100] = [
    32640, 32635, 32266, 31495, 30665, 29804, 29146, 28599, 28104, 27670, //
    27225, 26725, 26210, 25716, 25240, 24789, 24373, 23946, 23572, 22846, //
    21801, 20842, 19949, 19121, 18386, 17651, 16998, 16349, 15800, 15247, //
    14783, 14321, 13859, 13535, 13081, 12702, 12423, 12056, 11779, 11513, //
    11135, 10955, 10676, 10392, 10208, 9928, 9747, 9564, 9369, 9193, //
    9017, 8822, 8639, 8458, 8270, 8084, 7896, 7710, 7527, 7347, //
    7156, 6977, 6788, 6607, 6422, 6236, 6054, 5867, 5684, 5495, //
    5305, 5128, 4945, 4751, 4638, 4442, 4248, 4065, 3888, 3698, //
    3509, 3326, 3139, 2957, 2775, 2586, 2405, 2216, 2037, 1846, //
    1666, 1483, 1297, 1109, 927, 735, 554, 375, 201, 128,
];

/// Threshold tables for greyscale images (single quantization table).
const GRAY_HASH: [u32; 100] = [
    510, 505, 422, 380, 355, 338, 326, 318, 311, 305, //
    300, 297, 293, 291, 288, 286, 284, 283, 281, 280, //
    279, 278, 277, 273, 262, 251, 243, 233, 225, 218, //
    211, 205, 198, 193, 186, 181, 177, 172, 168, 164, //
    158, 156, 152, 148, 145, 142, 139, 136, 133, 131, //
    129, 126, 123, 120, 118, 115, 113, 110, 107, 105, //
    102, 100, 97, 94, 92, 89, 87, 83, 81, 79, //
    76, 74, 70, 68, 66, 63, 61, 57, 55, 52, //
    50, 48, 44, 42, 39, 37, 34, 31, 29, 26, //
    24, 21, 18, 16, 13, 11, 8, 6, 3, 2,
];

const GRAY_SUMS: [u32; 100] = [
    16320, 16315, 15946, 15277, 14655, 14073, 13623, 13230, 12859, 12560, //
    12240, 11861, 11456, 11081, 10714, 10360, 10027, 9679, 9368, 9056, //
    8680, 8331, 7995, 7668, 7376, 7084, 6823, 6562, 6345, 6125, //
    5939, 5756, 5571, 5421, 5240, 5086, 4976, 4829, 4719, 4616, //
    4463, 4393, 4280, 4166, 4092, 3980, 3909, 3835, 3755, 3688, //
    3621, 3541, 3467, 3396, 3323, 3247, 3170, 3096, 3021, 2952, //
    2874, 2804, 2727, 2657, 2583, 2509, 2437, 2362, 2290, 2211, //
    2136, 2068, 1996, 1915, 1858, 1773, 1692, 1620, 1552, 1477, //
    1398, 1326, 1251, 1179, 1109, 1031, 961, 884, 814, 736, //
    667, 592, 518, 441, 369, 292, 221, 151, 86, 64,
];

/// Estimates JPEG quality (1-100) from DQT segment payloads.
///
/// `dqt_list` holds raw DQT payloads indexed by table id; `None` entries are
/// skipped. Returns `None` when no table is present or the thresholds reject
/// the values (mirroring ExifTool returning undef).
pub fn estimate_quality_from_dqt_tables(dqt_list: &[Option<&[u8]>]) -> Option<i64> {
    let mut qtbl: Vec<&[u8]> = Vec::new();
    let mut sum: u32 = 0;

    'dqt: for dqt in dqt_list.iter().flatten() {
        let mut i = 1;
        while i + 64 <= dqt.len() {
            let qt = &dqt[i..i + 64];
            sum += qt.iter().map(|&v| v as u32).sum::<u32>();
            qtbl.push(qt);
            if qtbl.len() >= 4 {
                break 'dqt;
            }
            i += 65;
        }
    }

    if qtbl.is_empty() {
        return None;
    }

    let mut qval = qtbl[0][2] as u32 + qtbl[0][53] as u32;
    let (hash, sums) = if qtbl.len() > 1 {
        // color JPEG
        qval += qtbl[1][0] as u32 + qtbl[1][63] as u32;
        (&COLOR_HASH, &COLOR_SUMS)
    } else {
        // greyscale JPEG
        (&GRAY_HASH, &GRAY_SUMS)
    };

    for i in 0..100 {
        if qval < hash[i] && sum < sums[i] {
            continue;
        }
        if (qval <= hash[i] && sum <= sums[i]) || i >= 50 {
            return Some((i + 1) as i64);
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dqt(table_id: u8, value: u8) -> Vec<u8> {
        let mut d = vec![table_id];
        d.extend_from_slice(&[value; 64]);
        d
    }

    #[test]
    fn test_greyscale_all_16_matches_exiftool() {
        // ExifTool 13.55: -JPEGQualityEstimate => 87 for a single all-16 table
        let t0 = dqt(0, 16);
        let list = [Some(t0.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }

    #[test]
    fn test_color_two_tables() {
        // qval = 16+16+17+17 = 66, sum = 64*16 + 64*17 = 2112 -> quality 87
        let t0 = dqt(0, 16);
        let t1 = dqt(1, 17);
        let list = [Some(t0.as_slice()), Some(t1.as_slice()), None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }

    #[test]
    fn test_highest_quality_table() {
        // All-1s table: qval = 2, sum = 64 -> exact match at index 99 -> 100
        let t0 = dqt(0, 1);
        let list = [Some(t0.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(100));
    }

    #[test]
    fn test_no_tables_returns_none() {
        let list: [Option<&[u8]>; 4] = [None, None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), None);
    }

    #[test]
    fn test_short_segment_ignored() {
        let short = [0u8; 10];
        let list = [Some(&short[..]), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), None);
    }

    #[test]
    fn test_one_segment_two_tables_is_color() {
        // A single DQT segment may carry several 65-byte tables back to back.
        let mut seg = dqt(0, 16);
        seg.extend_from_slice(&dqt(1, 17));
        let list = [Some(seg.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }
}
