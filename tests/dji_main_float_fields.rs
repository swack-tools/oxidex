//! `DJI::Main` 0x0003..0x000b (`SpeedX` .. `CameraRoll`): inline `float`
//! values rendered with `sprintf("%+.2f")` (DJI.pm, pinned 13.59).
//!
//! Forward-ported from `origin/main` 95d16186 (#708), where the fix landed
//! after this branch diverged. The census that found it
//! (docs/reference/main-divergence-2026-09-18.md) counted these nine tags as
//! matched on main and MISSING on the tip in 11 combined-samples/DJI files.
//!
//! Expected values are the pinned oracle's:
//!
//! ```text
//! $ exiftool-pinned.sh -G1 -a -s -SpeedX -SpeedY -SpeedZ -Pitch -Yaw -Roll \
//!       -CameraPitch -CameraYaw -CameraRoll DJI_FC330.jpg DJI_Pocket.jpg
//! ======== DJI_FC330.jpg
//! [DJI] SpeedX +0.00, SpeedY +0.00, SpeedZ -0.30, Pitch -4.10, Yaw +32.00,
//!       Roll -2.10, CameraPitch -25.10, CameraYaw +31.90, CameraRoll +0.00
//! ======== DJI_Pocket.jpg
//! [DJI] SpeedX/Y/Z +0.00, Pitch/Yaw/Roll +0.00, CameraPitch -179.70,
//!       CameraYaw +2.70, CameraRoll +0.00
//! ```

use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

fn assert_fields(file: &str, expected: &[(&str, &str)]) {
    let Some(path) = fixtures::pinned_combined_fixture_path(&format!("DJI/{file}")) else {
        return;
    };
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    for (tag, value) in expected {
        assert_eq!(
            metadata.get_string(&format!("DJI:{tag}")),
            Some(*value),
            "{file} DJI:{tag}"
        );
    }
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/DJI"]
fn dji_main_float_fields_match_pinned_exiftool() {
    assert_fields(
        "DJI_FC330.jpg",
        &[
            ("SpeedX", "+0.00"),
            ("SpeedY", "+0.00"),
            ("SpeedZ", "-0.30"),
            ("Pitch", "-4.10"),
            ("Yaw", "+32.00"),
            ("Roll", "-2.10"),
            ("CameraPitch", "-25.10"),
            ("CameraYaw", "+31.90"),
            ("CameraRoll", "+0.00"),
        ],
    );
    assert_fields(
        "DJI_Pocket.jpg",
        &[
            ("SpeedZ", "+0.00"),
            ("Pitch", "+0.00"),
            ("CameraPitch", "-179.70"),
            ("CameraYaw", "+2.70"),
            ("CameraRoll", "+0.00"),
        ],
    );
}
