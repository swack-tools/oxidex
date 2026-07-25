//! DER X.509 certificate detection.

const ASN1_SEQUENCE: u8 = 0x30;
const ASN1_INTEGER: u8 = 0x02;
const ASN1_BIT_STRING: u8 = 0x03;
const ASN1_OID: u8 = 0x06;
const ASN1_UTC_TIME: u8 = 0x17;
const ASN1_GENERALIZED_TIME: u8 = 0x18;
const ASN1_CONTEXT_0: u8 = 0xA0;

#[derive(Clone, Copy)]
struct Tlv {
    tag: u8,
    value_start: usize,
    value_end: usize,
}

pub fn looks_like_der_x509(data: &[u8]) -> bool {
    let cert = match read_complete_tlv(data, 0, data.len(), ASN1_SEQUENCE) {
        Some(cert) => cert,
        None => return false,
    };

    let mut offset = cert.value_start;
    let tbs = match expect_complete_tlv(data, &mut offset, cert.value_end, ASN1_SEQUENCE) {
        Some(tbs) => tbs,
        None => return false,
    };

    let mut tbs_offset = tbs.value_start;
    if data.get(tbs_offset) == Some(&ASN1_CONTEXT_0)
        && expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_CONTEXT_0).is_none()
    {
        return false;
    }

    if expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_INTEGER).is_none() {
        return false;
    }

    let tbs_signature =
        match expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_SEQUENCE) {
            Some(algorithm) => algorithm,
            None => return false,
        };
    if !algorithm_identifier_has_oid(data, tbs_signature) {
        return false;
    }

    if expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_SEQUENCE).is_none() {
        return false;
    }

    let validity = match expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_SEQUENCE) {
        Some(validity) => validity,
        None => return false,
    };
    if !validity_has_two_times(data, validity) {
        return false;
    }

    if expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_SEQUENCE).is_none() {
        return false;
    }

    let subject_public_key_info =
        match expect_complete_tlv(data, &mut tbs_offset, tbs.value_end, ASN1_SEQUENCE) {
            Some(spki) => spki,
            None => return false,
        };
    if !subject_public_key_info_is_certificate_like(data, subject_public_key_info) {
        return false;
    }

    let cert_signature_algorithm =
        match expect_complete_tlv(data, &mut offset, cert.value_end, ASN1_SEQUENCE) {
            Some(algorithm) => algorithm,
            None => return false,
        };
    if !algorithm_identifier_has_oid(data, cert_signature_algorithm) {
        return false;
    }

    if expect_complete_tlv(data, &mut offset, cert.value_end, ASN1_BIT_STRING).is_none() {
        return false;
    }

    offset == cert.value_end
}

pub fn top_level_der_object_len(data: &[u8]) -> Option<usize> {
    if data.first() != Some(&ASN1_SEQUENCE) {
        return None;
    }

    let mut offset = 1;
    let length = read_length(data, &mut offset)?;
    offset.checked_add(length)
}

fn subject_public_key_info_is_certificate_like(data: &[u8], spki: Tlv) -> bool {
    let mut offset = spki.value_start;
    let algorithm = match expect_complete_tlv(data, &mut offset, spki.value_end, ASN1_SEQUENCE) {
        Some(algorithm) => algorithm,
        None => return false,
    };
    let public_key = match expect_complete_tlv(data, &mut offset, spki.value_end, ASN1_BIT_STRING) {
        Some(public_key) => public_key,
        None => return false,
    };
    let public_key_value_len = public_key
        .value_end
        .checked_sub(public_key.value_start)
        .unwrap_or(0);

    algorithm_identifier_has_oid(data, algorithm)
        && public_key_value_len > 1
        && data
            .get(public_key.value_start)
            .is_some_and(|unused_bits| *unused_bits <= 7)
}

fn validity_has_two_times(data: &[u8], validity: Tlv) -> bool {
    let mut offset = validity.value_start;
    read_time_tlv(data, &mut offset, validity.value_end).is_some()
        && read_time_tlv(data, &mut offset, validity.value_end).is_some()
}

fn read_time_tlv(data: &[u8], offset: &mut usize, limit: usize) -> Option<Tlv> {
    let tlv = read_tlv(data, *offset, limit)?;
    if tlv.tag != ASN1_UTC_TIME && tlv.tag != ASN1_GENERALIZED_TIME {
        return None;
    }
    if tlv.value_end > limit {
        return None;
    }
    *offset = tlv.value_end;
    Some(tlv)
}

fn algorithm_identifier_has_oid(data: &[u8], algorithm: Tlv) -> bool {
    algorithm_identifier_oid(data, algorithm).is_some()
}

fn algorithm_identifier_oid(data: &[u8], algorithm: Tlv) -> Option<Tlv> {
    let mut offset = algorithm.value_start;
    expect_complete_tlv(data, &mut offset, algorithm.value_end, ASN1_OID)
}

fn expect_complete_tlv(data: &[u8], offset: &mut usize, limit: usize, tag: u8) -> Option<Tlv> {
    let tlv = read_complete_tlv(data, *offset, limit, tag)?;
    *offset = tlv.value_end;
    Some(tlv)
}

fn read_complete_tlv(data: &[u8], offset: usize, limit: usize, tag: u8) -> Option<Tlv> {
    let tlv = read_tlv(data, offset, limit)?;
    if tlv.tag == tag && tlv.value_end <= limit {
        Some(tlv)
    } else {
        None
    }
}

fn read_tlv(data: &[u8], offset: usize, limit: usize) -> Option<Tlv> {
    if offset >= limit || offset + 2 > data.len() {
        return None;
    }

    let tag = data[offset];
    let mut cursor = offset + 1;
    let length = read_length(data, &mut cursor)?;
    let value_start = cursor;
    let value_end = value_start.checked_add(length)?;

    if value_end > data.len() {
        return None;
    }

    Some(Tlv {
        tag,
        value_start,
        value_end,
    })
}

fn read_length(data: &[u8], offset: &mut usize) -> Option<usize> {
    let first = *data.get(*offset)?;
    *offset += 1;

    if first & 0x80 == 0 {
        return Some(first as usize);
    }

    let octet_count = (first & 0x7F) as usize;
    if octet_count == 0 || octet_count > 4 || *offset + octet_count > data.len() {
        return None;
    }

    let mut length = 0usize;
    for _ in 0..octet_count {
        length = (length << 8) | data[*offset] as usize;
        *offset += 1;
    }
    Some(length)
}
