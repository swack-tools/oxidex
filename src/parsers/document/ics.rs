//! ICS (iCalendar) format parser
//!
//! Parses ICS (iCalendar) files to extract calendar metadata

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Parser for ICS (iCalendar) files
///
/// Extracts metadata from ICS calendar files including version, product ID,
/// calendar method, and counts of events and todos.
pub struct ICSParser;

impl ICSParser {
    /// Verifies the ICS file by checking for "BEGIN:VCALENDAR" and "VERSION:" markers
    pub fn verify_signature(data: &[u8]) -> bool {
        if let Ok(text) = std::str::from_utf8(data) {
            // ICS files must start with BEGIN:VCALENDAR and contain VERSION
            text.contains("BEGIN:VCALENDAR") && text.contains("VERSION:")
        } else {
            false
        }
    }

    /// Extracts a simple value from ICS format (KEY:VALUE)
    fn extract_value(text: &str, key: &str) -> Option<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(key) && trimmed.contains(':') {
                if let Some(value) = trimmed.strip_prefix(key) {
                    if let Some(val) = value.strip_prefix(':') {
                        return Some(val.trim().to_string());
                    }
                }
            }
        }
        None
    }

    /// Counts occurrences of a component type (e.g., VEVENT, VTODO)
    fn count_component(text: &str, component: &str) -> i64 {
        let begin_marker = format!("BEGIN:{}", component);
        text.lines()
            .filter(|line| line.trim() == begin_marker)
            .count() as i64
    }

    /// Extracts the first date found in the calendar
    fn extract_first_date(text: &str) -> Option<String> {
        // Look for DTSTART, DTEND, or other date fields
        let date_keys = ["DTSTART", "DTEND", "DTSTAMP", "CREATED", "LAST-MODIFIED"];

        for line in text.lines() {
            let trimmed = line.trim();
            for date_key in &date_keys {
                if trimmed.starts_with(date_key) && trimmed.contains(':') {
                    if let Some(value) = trimmed.split(':').nth(1) {
                        // Extract just the date part (YYYYMMDD or YYYYMMDDTHHMMSS)
                        let date_str = value.trim();
                        if !date_str.is_empty() && (date_str.len() == 8 || date_str.contains('T')) {
                            return Some(date_str.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Extracts the last date found in the calendar
    fn extract_last_date(text: &str) -> Option<String> {
        // Look for DTSTART, DTEND, or other date fields (in reverse)
        let date_keys = ["DTSTART", "DTEND", "DTSTAMP", "CREATED", "LAST-MODIFIED"];
        let mut last_date: Option<String> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            for date_key in &date_keys {
                if trimmed.starts_with(date_key) && trimmed.contains(':') {
                    if let Some(value) = trimmed.split(':').nth(1) {
                        let date_str = value.trim();
                        if !date_str.is_empty() && (date_str.len() == 8 || date_str.contains('T')) {
                            last_date = Some(date_str.to_string());
                        }
                    }
                }
            }
        }
        last_date
    }

    /// Splits an iCalendar content line into its `NAME *(";" param)` prefix
    /// and its value, at the first ":" that is not inside a double-quoted
    /// parameter value.
    ///
    /// RFC 5545 section 3.2 allows parameter values to be quoted strings
    /// which may themselves contain ":", ";", and ",". A naive
    /// `str::find(':')` finds the first colon anywhere in the line, so a
    /// line like:
    ///   `ORGANIZER;CN="Doe, John";DIR="ldap:ldap.example.com":mailto:jdoe@example.com`
    /// gets split inside the quoted DIR parameter value instead of at the
    /// real property/value boundary, corrupting the extracted value. This
    /// walks the line tracking quote state so the split lands on the
    /// correct colon.
    fn split_property_line(line: &str) -> Option<(&str, &str)> {
        let mut in_quotes = false;
        for (idx, ch) in line.char_indices() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ':' if !in_quotes => {
                    return Some((&line[..idx], &line[idx + 1..]));
                }
                _ => {}
            }
        }
        None
    }

    /// Unfolds RFC 5545 §3.1 folded content lines: a line that starts with a
    /// single space or tab is a continuation of the previous line and must
    /// be joined to it (with that one leading whitespace character dropped)
    /// before the line is parsed as a property. Real ExifTool's
    /// `ProcessVCard` (VCard.pm) does this same unfolding pass first; without
    /// it, a folded property (common in Apple/Google/Outlook exports) gets
    /// silently truncated at the fold point.
    fn unfold_lines(text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for raw_line in text.lines() {
            let line = raw_line.trim_end_matches('\r');
            if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
                out.last_mut().unwrap().push_str(&line[1..]);
            } else {
                out.push(line.to_string());
            }
        }
        out
    }

    /// Converts an iCalendar DATE or DATE-TIME value to ExifTool's
    /// "YYYY:MM:DD[ HH:MM:SS[Z]]" style, matching VCard.pm's `%timeInfo`
    /// ValueConv/PrintConv (which for ICS is effectively a passthrough
    /// reformat: `$self->ConvertDateTime($val)` on an already-EXIF-style
    /// string just returns it unchanged).
    fn convert_ics_datetime(val: &str) -> String {
        let bytes: Vec<char> = val.chars().collect();
        // YYYYMMDDTHHMMSS(Z?)
        if bytes.len() >= 15
            && bytes[8] == 'T'
            && bytes[..8].iter().all(|c| c.is_ascii_digit())
            && bytes[9..15].iter().all(|c| c.is_ascii_digit())
        {
            let z = if bytes.len() > 15 && bytes[15] == 'Z' {
                "Z"
            } else {
                ""
            };
            return format!(
                "{}:{}:{} {}:{}:{}{}",
                &val[0..4],
                &val[4..6],
                &val[6..8],
                &val[9..11],
                &val[11..13],
                &val[13..15],
                z
            );
        }
        // YYYYMMDD
        if bytes.len() == 8 && bytes.iter().all(|c| c.is_ascii_digit()) {
            return format!("{}:{}:{}", &val[0..4], &val[4..6], &val[6..8]);
        }
        // YYYY-MM-DD (leave any trailing time portion untouched)
        if bytes.len() >= 10
            && bytes[4] == '-'
            && bytes[7] == '-'
            && bytes[0..4].iter().all(|c| c.is_ascii_digit())
        {
            return format!(
                "{}:{}:{}{}",
                &val[0..4],
                &val[5..7],
                &val[8..10],
                &val[10..]
            );
        }
        val.to_string()
    }

    /// Maps a lower-cased iCalendar property name (as it appears directly
    /// under BEGIN:VCALENDAR) to its ExifTool tag name and whether the value
    /// needs `%timeInfo` date reformatting.
    ///
    /// Transcribed from `Image::ExifTool::VCard::VCalendar` (VCard.pm,
    /// ExifTool 13.59).
    fn vcalendar_tag_for(prop_lower: &str) -> Option<(&'static str, bool)> {
        Some(match prop_lower {
            "version" => ("VCalendarVersion", false),
            "calscale" => ("CalendarScale", false),
            "method" => ("Method", false),
            "prodid" => ("Software", false),
            "attach" => ("Attachment", false),
            "categories" => ("Categories", false),
            "class" => ("Classification", false),
            "comment" => ("Comment", false),
            "description" => ("Description", false),
            "geo" => ("Geolocation", false),
            "location" => ("Location", false),
            "percent-complete" => ("PercentComplete", false),
            "priority" => ("Priority", false),
            "resources" => ("Resources", false),
            "status" => ("Status", false),
            "summary" => ("Summary", false),
            "completed" => ("DateTimeCompleted", true),
            "dtend" => ("DateTimeEnd", true),
            "due" => ("DateTimeDue", true),
            "dtstart" => ("DateTimeStart", true),
            "duration" => ("Duration", false),
            "freebusy" => ("FreeBusyTime", false),
            "transp" => ("TimeTransparency", false),
            "tzid" => ("TimezoneID", false),
            "tzname" => ("TimezoneName", false),
            "tzoffsetfrom" => ("TimezoneOffsetFrom", false),
            "tzoffsetto" => ("TimezoneOffsetTo", false),
            "tzurl" => ("TimeZoneURL", false),
            "attendee" => ("Attendee", false),
            "contact" => ("Contact", false),
            "organizer" => ("Organizer", false),
            "recurrence-id" => ("RecurrenceID", false),
            "related-to" => ("RelatedTo", false),
            "url" => ("URL", false),
            "uid" => ("UID", false),
            "exdate" => ("ExceptionDateTimes", true),
            "rdate" => ("RecurrenceDateTimes", true),
            "rrule" => ("RecurrenceRule", false),
            "action" => ("Action", false),
            "repeat" => ("Repeat", false),
            "trigger" => ("Trigger", false),
            "created" => ("DateCreated", true),
            "dtstamp" => ("DateTimeStamp", true),
            "last-modified" => ("ModifyDate", true),
            "sequence" => ("SequenceNumber", false),
            "request-status" => ("RequestStatus", false),
            "acknowledged" => ("Acknowledged", true),
            // Observed X-tags (ref VCard.pm)
            "x-apple-calendar-color" => ("CalendarColor", false),
            "x-apple-default-alarm" => ("DefaultAlarm", false),
            "x-apple-local-default-alarm" => ("LocalDefaultAlarm", false),
            "x-microsoft-cdo-appt-sequence" => ("AppointmentSequence", false),
            "x-microsoft-cdo-ownerapptid" => ("OwnerAppointmentID", false),
            "x-microsoft-cdo-busystatus" => ("BusyStatus", false),
            "x-microsoft-cdo-intendedstatus" => ("IntendedBusyStatus", false),
            "x-microsoft-cdo-alldayevent" => ("AllDayEvent", false),
            "x-microsoft-cdo-importance" => ("Importance", false),
            "x-microsoft-cdo-insttype" => ("InstanceType", false),
            "x-microsoft-donotforwardmeeting" => ("DoNotForwardMeeting", false),
            "x-microsoft-disallow-counter" => ("DisallowCounterProposal", false),
            "x-microsoft-locations" => ("MeetingLocations", false),
            "x-wr-caldesc" => ("CalendarDescription", false),
            "x-wr-calname" => ("CalendarName", false),
            "x-wr-relcalid" => ("CalendarID", false),
            "x-wr-timezone" => ("TimeZone2", false),
            "x-wr-alarmuid" => ("AlarmUID", false),
            _ => return None,
        })
    }

    /// PrintConv for X-microsoft-cdo-importance (VCard.pm).
    fn importance_print_conv(val: &str) -> Option<&'static str> {
        match val.trim() {
            "0" => Some("Low"),
            "1" => Some("Normal"),
            "2" => Some("High"),
            _ => None,
        }
    }

    /// PrintConv for X-microsoft-cdo-insttype (VCard.pm).
    fn insttype_print_conv(val: &str) -> Option<&'static str> {
        match val.trim() {
            "0" => Some("Non-recurring Appointment"),
            "1" => Some("Recurring Appointment"),
            "2" => Some("Single Instance of Recurring Appointment"),
            "3" => Some("Exception to Recurring Appointment"),
            _ => None,
        }
    }

    /// Emits `VCard:<TagName>` entries for iCalendar properties that are
    /// direct children of BEGIN:VCALENDAR (nesting depth 1), matching real
    /// ExifTool's family-0 "VCard" group and VCard.pm's tag naming.
    /// Properties inside nested components (VEVENT, VALARM, VTIMEZONE, ...)
    /// are skipped - see comment at the call site.
    fn extract_vcalendar_tags(text: &str, metadata: &mut MetadataMap) {
        let mut depth: i32 = 0;
        for line in Self::unfold_lines(text) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("BEGIN:") {
                let _ = rest;
                depth += 1;
                continue;
            }
            if line.starts_with("END:") {
                depth -= 1;
                continue;
            }
            // Only properties directly under BEGIN:VCALENDAR (depth 1)
            if depth != 1 {
                continue;
            }
            // Property line: NAME[;PARAM=VAL...]:VALUE
            let Some((name_and_params, value)) = Self::split_property_line(line) else {
                continue;
            };
            let prop_name = name_and_params.split(';').next().unwrap_or(name_and_params);
            let prop_lower = prop_name.to_ascii_lowercase();

            let Some((tag_name, is_time)) = Self::vcalendar_tag_for(&prop_lower) else {
                continue;
            };

            let mut out_value = if is_time {
                Self::convert_ics_datetime(value.trim())
            } else if prop_lower == "geo" {
                value
                    .trim()
                    .strip_prefix("geo:")
                    .unwrap_or(value.trim())
                    .to_string()
            } else {
                value.trim().to_string()
            };

            if prop_lower == "x-microsoft-cdo-importance" {
                if let Some(pretty) = Self::importance_print_conv(&out_value) {
                    out_value = pretty.to_string();
                }
            } else if prop_lower == "x-microsoft-cdo-insttype" {
                if let Some(pretty) = Self::insttype_print_conv(&out_value) {
                    out_value = pretty.to_string();
                }
            }

            let key = format!("VCard:{}", tag_name);
            metadata.insert(key, TagValue::new_string(out_value));
        }
    }
}

impl FormatParser for ICSParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Read file data
        let file_size = reader.size() as usize;
        let data = reader.read(0, file_size)?;

        // Verify ICS signature
        if !Self::verify_signature(data) {
            return Err(ExifToolError::parse_error("Invalid ICS signature"));
        }

        // Convert to UTF-8 string
        let text = std::str::from_utf8(data)
            .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in ICS file"))?;

        let mut metadata = MetadataMap::new();

        // Set basic file info
        metadata.insert("FileType".to_string(), TagValue::String("ICS".to_string()));
        metadata.insert("FileSize".to_string(), TagValue::Integer(file_size as i64));
        metadata.insert(
            "MIMEType".to_string(),
            TagValue::String("text/calendar".to_string()),
        );

        // Extract VERSION (ICS:Version) - Worker 27 requirement
        if let Some(version) = Self::extract_value(text, "VERSION") {
            metadata.insert("ICS:Version".to_string(), TagValue::new_string(version));
        }

        // Extract PRODID (ICS:ProductID) - Worker 27 requirement
        if let Some(prodid) = Self::extract_value(text, "PRODID") {
            metadata.insert("ICS:ProductID".to_string(), TagValue::new_string(prodid));
        }

        // Extract CALSCALE (ICS:CalScale) - Worker 27 requirement
        if let Some(calscale) = Self::extract_value(text, "CALSCALE") {
            metadata.insert("ICS:CalScale".to_string(), TagValue::new_string(calscale));
        }

        // Extract METHOD (ICS:Method) - Worker 27 requirement
        if let Some(method) = Self::extract_value(text, "METHOD") {
            metadata.insert("ICS:Method".to_string(), TagValue::new_string(method));
        }

        // Count VEVENT entries (ICS:EventCount) - Worker 27 requirement
        let event_count = Self::count_component(text, "VEVENT");
        if event_count > 0 {
            metadata.insert(
                "ICS:EventCount".to_string(),
                TagValue::new_integer(event_count),
            );
        }

        // Count VTODO entries (ICS:TodoCount) - Worker 27 requirement
        let todo_count = Self::count_component(text, "VTODO");
        if todo_count > 0 {
            metadata.insert(
                "ICS:TodoCount".to_string(),
                TagValue::new_integer(todo_count),
            );
        }

        // Extract first date (ICS:FirstDate) - Worker 27 requirement
        if let Some(first_date) = Self::extract_first_date(text) {
            metadata.insert(
                "ICS:FirstDate".to_string(),
                TagValue::new_string(first_date),
            );
        }

        // Extract last date (ICS:LastDate) - Worker 27 requirement
        if let Some(last_date) = Self::extract_last_date(text) {
            metadata.insert("ICS:LastDate".to_string(), TagValue::new_string(last_date));
        }

        // Real ExifTool parity: ICS files are read by ExifTool's VCard.pm module
        // (Image::ExifTool::VCard::VCalendar table), which puts every extracted
        // tag under family-0 group "VCard" - NOT "ICS". The `ICS:*` tags above
        // are a fabricated namespace with no counterpart in real ExifTool output;
        // they're left in place only because existing tests assert on them.
        //
        // This adds the real `VCard:<TagName>` tags for properties that are
        // direct children of BEGIN:VCALENDAR (depth 1), matching
        // Image::ExifTool::VCard::VCalendar. Verified against ExifTool 13.59
        // (`exiftool -G -s`) on t/images/VCard.ics. Properties nested inside
        // VEVENT/VALARM/VTIMEZONE etc. are intentionally NOT emitted here:
        // ExifTool disambiguates repeated tag names (e.g. multiple VEVENTs)
        // using family-1 group numbering (Event1, Event2, ...), which this
        // flat Group:Tag map cannot represent without risking collisions/
        // silently-wrong values, so those are left as a documented gap.
        Self::extract_vcalendar_tags(text, &mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ICS)
    }
}

/// Parses metadata from ICS files.
///
/// This is a convenience wrapper around ICSParser that provides a functional API.
pub fn parse_ics_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = ICSParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;

    #[test]
    fn test_ics_basic_parsing() {
        let ics_data = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//Test//EN\r\nCALSCALE:GREGORIAN\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nDTSTART:20240101T120000Z\r\nDTEND:20240101T130000Z\r\nSUMMARY:Test Event\r\nEND:VEVENT\r\nEND:VCALENDAR";

        let reader = BufferedReader::from_bytes(ics_data);
        let parser = ICSParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("FileType").unwrap().as_string(), Some("ICS"));
        assert_eq!(
            metadata.get("ICS:Version").unwrap().as_string(),
            Some("2.0")
        );
        assert_eq!(
            metadata.get("ICS:ProductID").unwrap().as_string(),
            Some("-//Test//Test//EN")
        );
        assert_eq!(
            metadata.get("ICS:CalScale").unwrap().as_string(),
            Some("GREGORIAN")
        );
        assert_eq!(
            metadata.get("ICS:Method").unwrap().as_string(),
            Some("PUBLISH")
        );
        assert_eq!(
            metadata.get("ICS:EventCount").unwrap().as_integer(),
            Some(1)
        );
    }

    #[test]
    fn test_ics_invalid() {
        let invalid_data = b"Not an ICS file";
        let reader = BufferedReader::from_bytes(invalid_data);
        let parser = ICSParser;

        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn split_property_line_ignores_colons_inside_quoted_params() {
        // RFC 5545 quoted parameter values may contain ":" (e.g. a "ldap:"
        // URI), which must not be mistaken for the property/value delimiter.
        let line =
            r#"ORGANIZER;CN="Doe, John";DIR="ldap:ldap.example.com":mailto:jdoe@example.com"#;
        let (name_and_params, value) =
            ICSParser::split_property_line(line).expect("line should split");
        assert_eq!(
            name_and_params,
            r#"ORGANIZER;CN="Doe, John";DIR="ldap:ldap.example.com""#
        );
        assert_eq!(value, "mailto:jdoe@example.com");
    }

    #[test]
    fn test_ics_vcalendar_tags_survive_quoted_params() {
        let ics_data = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nORGANIZER;CN=\"Doe, John\";DIR=\"ldap:ldap.example.com\":mailto:jdoe@example.com\r\nEND:VCALENDAR";
        let reader = BufferedReader::from_bytes(ics_data);
        let parser = ICSParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("VCard:Organizer").unwrap().as_string(),
            Some("mailto:jdoe@example.com")
        );
    }

    #[test]
    fn test_ics_unfolds_rfc5545_continuation_lines() {
        // Verified against pinned ExifTool 13.59: a folded X-WR-CALNAME
        // (space/tab-prefixed continuation lines, RFC 5545 section 3.1)
        // must be rejoined before parsing, or the value truncates at the
        // fold point. `exiftool -G -s` on this same content reports
        // CalendarName as the fully-joined string below, byte-for-byte.
        let ics_data = b"BEGIN:VCALENDAR\r\nCALSCALE:GREGORIAN\r\nVERSION:2.0\r\nMETHOD:PUBLISH\r\nX-WR-CALNAME:This is a long calendar name that would fold\r\n across multiple content lines per RFC 5545 section 3.1\r\n because it exceeds the seventy-five octet limit.\r\nEND:VCALENDAR\r\n";
        let reader = BufferedReader::from_bytes(ics_data);
        let parser = ICSParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("VCard:CalendarName").unwrap().as_string(),
            Some(
                "This is a long calendar name that would foldacross multiple content lines per RFC 5545 section 3.1because it exceeds the seventy-five octet limit."
            )
        );
    }
}
