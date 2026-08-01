//! Integration tests for the Sigma lens-name database.
//!
//! The MakerNote tests that used to live here drove a registry-backed
//! `SigmaMakerNoteParser` and asserted the names it invented -- `LensID` at
//! 0x001b, `ColorMode` at 0x001e, numeric PrintConvs for tags `Sigma.pm`
//! stores as strings. None of that agreed with `Sigma.pm`, so both the parser
//! and these tests were removed. `parsers::tiff::makernotes::sigma` is now the
//! single Sigma MakerNote table; it is transcribed from `Sigma.pm` and carries
//! its own unit tests, and both callers -- Sigma JPEGs and the JpgFromRaw
//! preview inside an X3F -- are covered by the ExifTool comparison harness.

#[test]
fn test_sigma_lens_database_art_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series prime lenses
    assert_eq!(
        lookup_lens_name(1),
        Some("Sigma 14mm f/1.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(3),
        Some("Sigma 24mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(6),
        Some("Sigma 35mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(10),
        Some("Sigma 50mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(13),
        Some("Sigma 85mm f/1.4 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_telephoto() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series telephoto primes
    assert_eq!(
        lookup_lens_name(15),
        Some("Sigma 105mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(16),
        Some("Sigma 135mm f/1.8 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_macro() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series macro lenses
    assert_eq!(
        lookup_lens_name(20),
        Some("Sigma 70mm f/2.8 DG Macro Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(21),
        Some("Sigma 105mm f/2.8 DG DN Macro Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series zoom lenses
    assert_eq!(
        lookup_lens_name(30),
        Some("Sigma 14-24mm f/2.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(31),
        Some("Sigma 18-35mm f/1.8 DC HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(33),
        Some("Sigma 24-70mm f/2.8 DG OS HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(35),
        Some("Sigma 50-100mm f/1.8 DC HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series primes
    assert_eq!(
        lookup_lens_name(50),
        Some("Sigma 16mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(51),
        Some("Sigma 23mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(52),
        Some("Sigma 30mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(53),
        Some("Sigma 56mm f/1.4 DC DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series zoom lenses
    assert_eq!(
        lookup_lens_name(54),
        Some("Sigma 17-70mm f/2.8-4.0 DC Macro OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(57),
        Some("Sigma 100-400mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(58),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_sports_series() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Sports series lenses
    assert_eq!(
        lookup_lens_name(70),
        Some("Sigma 120-300mm f/2.8 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(71),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(72),
        Some("Sigma 500mm f/4.0 DG OS HSM Sports".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount zoom lenses
    assert_eq!(
        lookup_lens_name(100),
        Some("Sigma 8-16mm f/4.5-5.6 DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(102),
        Some("Sigma 17-50mm f/2.8 EX DC OS HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(107),
        Some("Sigma 50-500mm f/4.5-6.3 APO DG OS HSM".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount prime lenses
    assert_eq!(
        lookup_lens_name(120),
        Some("Sigma 8mm f/3.5 EX DG Circular Fisheye".to_string())
    );

    assert_eq!(
        lookup_lens_name(123),
        Some("Sigma 30mm f/1.4 EX DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(125),
        Some("Sigma 180mm f/2.8 EX DG OS HSM APO Macro".to_string())
    );
}

#[test]
fn test_sigma_lens_database_dg_dn_mirrorless() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma DG DN mirrorless lenses
    assert_eq!(
        lookup_lens_name(150),
        Some("Sigma 14-24mm f/2.8 DG DN Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(151),
        Some("Sigma 20mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(154),
        Some("Sigma 35mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(157),
        Some("Sigma 90mm f/2.8 DG DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_not_found() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test that unknown lens IDs return None
    assert_eq!(lookup_lens_name(9999), None);
    assert_eq!(lookup_lens_name(0), None);
    assert_eq!(lookup_lens_name(500), None);
}
