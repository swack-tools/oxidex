#!/usr/bin/env python3
"""Compile the bounded ProcessMOV direct-Format UserData protocol.

This is an exact, reviewed native protocol implementation, not arbitrary Perl
translation. Unknown executable statements, helper bindings, caller controls,
and row controls refuse. Declarative names, formats, enum and charset map
operands are compiled. Movie-level moov/udta only; IText and implicit Format
remain explicit omissions.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import quicktime_atom_tables as selector
from quicktime_generated_specs import (EXPECTED_PROCESSOR, EXPECTED_PROCESSOR_DEPARSE_SHA256,
    EXPECTED_READER_PROTOCOL, source_format, rust_string, render_format, itemlist_use,
    require_nonempty_supported)

ROOT = selector.ROOT
SNAPSHOT = ROOT / 'tools/exiftool-tables/fixtures/quicktime_source_13_59.json'
LEDGER = ROOT / 'tools/exiftool-tables/quicktime_generated_userdata_ledger.json'
RUST = ROOT / 'src/parsers/quicktime/generated_userdata_specs.rs'
# Core source binds ReadValue's private size/procedure registry, option defaults
# and FoundTag priority defaults in addition to the executable helper bodies.
# Bounded capture differs only in ampersands on two trailer helper calls
# whose prototypes have not yet loaded; both complete native bodies reviewed.
PROCESSOR_HASHES = (EXPECTED_PROCESSOR_DEPARSE_SHA256, '75e2459dd90d641982d657ad29835470e9753f17c144ce1332f86f00c7e33522')
PROCESSOR_HASHES_BY_VERSION = {
    '11.78': ('f70ea96a1287580357cc67365144b20ea273b47fc2de37baf24b8bf26ccb5303',),
    '12.64': ('3121d19e978bbaf944c0d9698ce597234bb5dbb889dc2c1e4d3e42b11c02b58d',),
    '13.59': PROCESSOR_HASHES,
}
CORE_SHA = '95fa4ec3cc3603866dd6e37bfe52ad019ff50a23bbd5cc87ce40f949cf49a508'
CHARSET_SHA = '2017febff0262d7e0d7ea2325482ed62f9c9461e2331a36ef8fda7c723865e0e'
XMP_SHA = '1e3612f54b7dd3a08cb326b22fdaa72e09f5a78b3d819cb093174e7567a2508b'
HELPERS = {name: (hashes, 'Image/ExifTool/Charset.pm' if '::Charset::' in name else 'Image/ExifTool.pm', CHARSET_SHA if '::Charset::' in name else CORE_SHA)
           for name, hashes in EXPECTED_READER_PROTOCOL['dependencies'].values()
           if not name.endswith('::QuickTimeFormat')}
HELPERS.update({
    'Image::ExifTool::IsUTF8': (('ffa51f969f6446605ecc6f3e452b31816545748dc4e4b24684de1e12537f15e6',), 'Image/ExifTool.pm', CORE_SHA),
    'Image::ExifTool::FoundTag': (('b6bbdc0ace76a1a371da446a8c5b880e6ef75c5630d23b7e3571e018f74e08e4',), 'Image/ExifTool.pm', CORE_SHA),
    'Image::ExifTool::XMP::FixUTF8': (('bbc6ea4b68b3c59f5834a74696b1523ae0eda2ae8800e07fd9d72227acf91f16',), 'Image/ExifTool/XMP.pm', XMP_SHA),
})
# The direct movie-level UserData Format branch passes the complete atom to
# ReadValue in both 12.64 and 13.59. Its text path still uses IsUTF8, Decode,
# the loaded MacRoman map and FoundTag. 12.64 FoundTag differs in IgnoreGroups
# and document-number/duplicate bookkeeping; this subset uses neither
# IgnoreGroups nor DOC_NUM, and priority is supplied by the selected row.
# Bind every helper to its version's selected-library bytes and reviewed body.
HISTORICAL_1264_SOURCE_SHA = {
    'Image/ExifTool.pm': 'c2d4fb2246bb0d0c27adaedc1d2eb109967224ea4c1ee5d23009c5186cb3faf2',
    'Image/ExifTool/Charset.pm': 'a323c2e1a7188250e3d6ffdf30c901862787390aaf78deaa8d763d8be83cbd48',
    'Image/ExifTool/XMP.pm': '120b31867a35a45f68b25ce9212be36cbf08c6f0be9b0870d5e3a06aeb6b01ea',
}
HISTORICAL_1264_FOUNDTAG_SHA = '556d3e68356a16a4c4b226cdc2960913963db8519293ce8f2ac686269d8a1718'
HISTORICAL_1178_SOURCE_SHA = {
    'Image/ExifTool.pm': 'caa6fe4ac1ab1bf7be369caffac635b460be8dcf1943354ff6a68a9f789f23aa',
    'Image/ExifTool/Charset.pm': 'c380b6f66da803852b3ffa50554062ffbdadfc152d672f0bc0e837ecdb2d2da6',
    'Image/ExifTool/XMP.pm': 'de5c1713333ee444ec89a8d3c358a87399d2b4f0c544e5be6f3c9a61ecac9350',
}
HISTORICAL_1178_FOUNDTAG_SHA = '8e785d704bef4eeeb84841df46edb8970a98d6732ff495ea3a02657e1712c52e'
HISTORICAL_1178_XMP_ISUTF8_SHA = '68bb2cb70b3b2570170992662107a17c60818f278a09c01d4792892382e2ac32'

def helpers_for_version(version):
    if version == '13.59':
        return HELPERS
    if version == '11.78':
        # ProcessMOV calls XMP::IsUTF8 before that helper moved to the core.
        # The core fact is recorded as unavailable; validate it separately.
        helpers = {name: ((HISTORICAL_1178_FOUNDTAG_SHA,) if name == 'Image::ExifTool::FoundTag' else hashes,
                          file, HISTORICAL_1178_SOURCE_SHA[file])
                   for name, (hashes, file, _) in HELPERS.items()
                   if name != 'Image::ExifTool::IsUTF8'}
        helpers['Image::ExifTool::XMP::IsUTF8'] = ((HISTORICAL_1178_XMP_ISUTF8_SHA,),
            'Image/ExifTool/XMP.pm', HISTORICAL_1178_SOURCE_SHA['Image/ExifTool/XMP.pm'])
        return helpers
    if version != '12.64':
        return {}
    return {name: ((HISTORICAL_1264_FOUNDTAG_SHA,) if name == 'Image::ExifTool::FoundTag' else hashes,
                   file, HISTORICAL_1264_SOURCE_SHA[file])
            for name, (hashes, file, _) in HELPERS.items()}

def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()

def protocol_reason(doc, table):
    proc = table['meta'].get('PROCESS_PROC', {})
    version = doc.get('exiftool_version')
    processor_hashes = PROCESSOR_HASHES_BY_VERSION.get(version, ())
    if (any(proc.get(k) != v for k,v in EXPECTED_PROCESSOR.items())
        or not isinstance(proc.get('__deparse'), str)
        or hashlib.sha256(proc['__deparse'].encode()).hexdigest() not in processor_hashes
        or proc.get('source_file') != 'Image/ExifTool/QuickTime.pm'
        or not isinstance(proc.get('source_sha256'), str) or len(proc['source_sha256']) != 64):
        return 'unsupported_userdata_processor'
    meta = table['meta']
    # SetupTagTable adds these two identity keys to the hydrated table. The
    # widened dump now classifies them as table metadata. Accept only the
    # source-proved identity for this exact table; a changed value still
    # refuses the bounded reader contract.
    identity_meta = {'TABLE_NAME': 'Image::ExifTool::QuickTime::UserData',
                     'SHORT_NAME': 'QuickTime::UserData'}
    if (meta.get('FORMAT') != 'string' or meta.get('GROUPS', {}).get('1') != 'UserData'
        or meta.get('GROUPS', {}).get('0', 'QuickTime') != 'QuickTime'
        or any(meta.get(key) != value for key, value in identity_meta.items() if key in meta)
        or set(meta) - {'CHECK_PROC','FORMAT','GROUPS','LANG_INFO','NOTES','PREFERRED','PROCESS_PROC','TAG_PREFIX','WRITABLE','WRITE_GROUP','WRITE_PROC', *identity_meta}):
        return 'unsupported_userdata_table_controls'
    p = doc.get('quicktime_userdata_reader_protocol', {})
    if p.get('kind') != 'quicktime_userdata_reader_protocol_v1':
        return 'missing_userdata_protocol'
    if p.get('caller_meta') != {'Main':{'GROUPS':{'2':'Video'}}, 'Movie':{'GROUPS':{'2':'Video'}}}:
        return 'unsupported_userdata_caller_meta'
    if set(p.get('caller_processors', {})) != {'Main','Movie'}:
        return 'unsupported_userdata_caller_processors'
    for name, fact in p['caller_processors'].items():
        if (any(fact.get(k) != v for k,v in EXPECTED_PROCESSOR.items())
            or fact.get('resolved') is not True
            or fact.get('source_file') != proc['source_file'] or fact.get('source_sha256') != proc['source_sha256']
            or not isinstance(fact.get('__deparse'),str)
            or hashlib.sha256(fact['__deparse'].encode()).hexdigest() not in processor_hashes):
            return 'unsupported_userdata_caller_processor:'+name
    for name, target in [('main_movie_edge','Movie'), ('movie_userdata_edge','UserData')]:
        if p.get(name) != {'Name':target, 'SubDirectory':{'TagTable':'Image::ExifTool::QuickTime::'+target}}:
            return 'unsupported_userdata_caller:'+name
    deps = p.get('dependencies', {})
    helpers = helpers_for_version(version)
    if version == '11.78':
        unavailable = deps.get('Image::ExifTool::IsUTF8', {})
        if unavailable.get('resolved') is not False or unavailable.get('reason') != 'code_ref_unavailable':
            return 'unsupported_userdata_utf8_binding'
    expected_names = set(helpers) | ({'Image::ExifTool::IsUTF8'} if version == '11.78' else set())
    if set(deps) != expected_names:
        return 'unsupported_userdata_dependencies'
    for name,(hashes,file,sha) in helpers.items():
        f = deps[name]
        if (not isinstance(f,dict) or f.get('__name') != name or f.get('__perl') != 'CODE'
            or f.get('__opaque') is not True or f.get('resolved') is not True
            or f.get('source_file') != file or f.get('source_sha256') != sha
            or not isinstance(f.get('__deparse'),str)
            or hashlib.sha256(f['__deparse'].encode()).hexdigest() not in hashes):
            return 'unsupported_userdata_helper:'+name
    if p.get('default_charset') != 'MacRoman' or p.get('charset_type') != 0x101:
        return 'unsupported_userdata_charset'
    m = p.get('charset_map', {})
    operands = m.get('operands')
    if (m.get('resolved') is not True or m.get('charset') != p['default_charset']
        or m.get('source_file') != 'Image/ExifTool/Charset/MacRoman.pm'
        or not isinstance(m.get('source_sha256'),str) or len(m['source_sha256']) != 64
        or not isinstance(operands,dict) or m.get('map_sha256') != digest(operands)):
        return 'unsupported_userdata_charset_map'
    # Exact Decompose's byte-index lookup with identity fallback. No Python or
    # platform codec table participates in generation or runtime.
    if any(not k.isdigit() or str(int(k)) != k or not 0 <= int(k) <= 255
           or type(v) is not int or not 0 <= v <= 0x10ffff or 0xd800 <= v <= 0xdfff
           for k,v in operands.items()):
        return 'unsupported_userdata_charset_operands'
    return None

def compile_document(doc):
    if doc.get('exiftool_version') != (ROOT/'.exiftool-version').read_text().strip():
        raise ValueError('source differs from repository pin')
    if 'hydrated_layouts' in doc:
        from capture_quicktime_baseline import hydrated_tables
        tables = hydrated_tables(doc)
        doc = {**doc, 'modules':{**doc['modules'], 'QuickTime':{**doc['modules']['QuickTime'],'tables':tables,'table_count':len(tables)}}}
    family = next(x for x in selector.inventory(doc)['families'] if x['table']=='UserData')
    table = doc['modules']['QuickTime']['tables']['UserData']
    blocked = protocol_reason(doc,table)
    specs=[];ledger=[]
    for record in family['records']:
        reasons=[r for r in record['reasons'] if r!='protocol_userdata_language_records']
        row=record['refused_source']
        if row.get('Format') is None: reasons.append('unsupported_userdata_implicit_format')
        if row.get('Format') == 'int64u': reasons.append('unsupported_userdata_uint64_json_representation')
        # FoundTag honors Avoid as a priority operand, not just writer metadata.
        if str(row.get('Avoid','0')) not in ('0','1'): reasons.append('unsupported_userdata_priority')
        if blocked: reasons.append(blocked)
        reasons=sorted(set(reasons))
        entry={'identity':record['identity'],'generated':not reasons,'reasons':reasons}
        if not reasons:
            raw=record['identity']['raw_key'].encode('latin1')
            group=row.get('Groups',{})
            spec={'raw_fourcc':raw.hex(),'name':row['Name'],
                  'group':group.get('1',table['meta']['GROUPS']['1']),
                  'group0':group.get('0',table['meta']['GROUPS'].get('0','QuickTime')),
                  'priority':0 if str(row.get('Avoid','0'))=='1' else 1,
                  'source_format':source_format(row['Format']),
                  'safe_enum_operands':[{'raw':k,'rendered':v} for k,v in sorted(row.get('PrintConv',{}).get('map',{}).items())],
                  'source_identity':record['identity']}
            specs.append(spec);entry['generated_spec_sha256']=digest(spec)
        ledger.append(entry)
    specs.sort(key=lambda x:x['raw_fourcc'])
    return {'schema':'quicktime_generated_userdata_specs_v1',
            'scope':'movie-level moov/udta explicit direct Format; language, implicit and custom controls omitted',
            'source':{'exiftool_version':doc['exiftool_version'],'table_sha256':family['source_table_sha256']},
            'protocol':{'eligible':blocked is None,'reason':blocked,'capture':doc.get('quicktime_userdata_reader_protocol'),
                        'processor':table['meta'].get('PROCESS_PROC'),
                        'operands':{'auto_decode_max_bytes':65536,'copyright_prefix':169,'byte_order':'MM'}},
            'specs':specs,'ledger':ledger,
            'identity_counts':{'source_records':len(ledger),'generated':len(specs),'omitted':len(ledger)-len(specs)}}

def render_rust(r):
    lines=['#[derive(Clone, Copy, Debug)]',
           '#[rustfmt::skip]',
           'pub(crate) struct UserDataSpec { pub data: ItemListSpec, pub priority: u8 }',
           'pub(crate) const AUTO_DECODE_MAX_BYTES: usize = 65536;',
           'pub(crate) const COPYRIGHT_PREFIX: u8 = 169;',
           '#[rustfmt::skip]', 'pub(crate) static USERDATA_SPECS: &[UserDataSpec] = &[']
    for x in r['specs']:
        raw=', '.join('0x'+x['raw_fourcc'][i:i+2] for i in range(0,8,2))
        enums=', '.join('EnumOperand { raw: %s, rendered: %s }'%(rust_string(e['raw']),rust_string(e['rendered'])) for e in x['safe_enum_operands'])
        lines.append('    UserDataSpec { data: ItemListSpec { raw_fourcc: [%s], name: %s, group: %s, group0: %s, source_format: %s, safe_enum_operands: &[%s] }, priority: %d },'%(raw,rust_string(x['name']),rust_string(x['group']),rust_string(x['group0']),render_format(x['source_format']),enums,x['priority']))
    lines += ['];', '#[rustfmt::skip]', 'pub(crate) static SINGLE_BYTE_MAP: &[u32; 256] = &[']
    m=r['protocol']['capture']['charset_map']['operands'] if r['protocol']['eligible'] else {}
    for i in range(0,256,16):lines.append('    '+', '.join(str(m.get(str(j),j)) for j in range(i,i+16))+',')
    body='\n'.join(lines+['];',''])
    return '\n'.join(['// @generated by tools/exiftool-tables/quicktime_userdata_specs.py; DO NOT EDIT.',
                      *itemlist_use(body), body])

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--dump',type=Path,default=SNAPSHOT);p.add_argument('--ledger',type=Path,default=LEDGER);p.add_argument('--rust',type=Path,default=RUST);p.add_argument('--replace',action='store_true');p.add_argument('--check',action='store_true');a=p.parse_args()
    r=compile_document(json.loads(a.dump.read_text()))
    require_nonempty_supported(r, 'UserData')
    for path,text in [(a.ledger,json.dumps(r,sort_keys=True,indent=2,ensure_ascii=False)+'\n'),(a.rust,render_rust(r))]:
        if a.check:
            if not path.is_file() or path.read_text()!=text:p.error('stale UserData artifact: '+str(path))
        elif path.exists() and not a.replace:p.error('output exists: '+str(path))
        else:path.write_text(text)
if __name__=='__main__':main()
