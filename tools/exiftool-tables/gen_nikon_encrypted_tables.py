#!/usr/bin/env python3
"""Generate Nikon's encrypted binary tables from the loaded ExifTool dump.

This is deliberately a small, audited translator for the runtime DSL in
``nikon/binary_data.rs``.  It never reads Rust table output: names, offsets,
enum labels, table graph, and root alternatives all come from the loaded
Nikon/NikonCustom declarations.  A field or expression outside that DSL is a
hard error before the destination is opened.
"""
from __future__ import annotations

import argparse, hashlib, json, re, sys
from pathlib import Path

import conds


class Unsupported(ValueError): pass


# The runtime's state vocabulary.  Keeping it closed makes a newly introduced
# self member an explicit parser/runtime decision rather than guessed output.
DMS = {x: x for x in '''AFAreaMode AfAreaInitialHeight AfAreaInitialWidth AutoCapturedFrame BracketSet CmdDialsReverseRotExposureComp DynamicAFAreaSize FirmwareVersion FlashControlBuiltin FlashControlMode FlashGroupOptionsMasterMode FocusDistanceRangeWidth FocusMode FocusShiftNumberShots FocusShiftShooting FocusStepsFromInfinity HDMIBitDepth HDMIOutputNLog HDR ImageArea IntervalFrame IntervalShooting IntervalShootingIntervals IntervalShootingShotsPerInterval LensDriveEnd LensID MovieType MultipleExposureMode NewLensData OldLensData PixelShiftActive PixelShiftShooting ShotInfoVersion ShutterMode SingleFrame ZebraPatternToneRange'''.split()}
FORMATS = {'int8u':'U8','int8s':'I8','int16u':'U16','int16s':'I16','int32u':'U32','int32s':'I32','fixed32u':'Fixed32u','string':'Str','undef':'Undef'}
ALLOWED = {'Name','Description','Notes','Format','Condition','RawConv','ValueConv','PrintConv','ValueConvInv','PrintConvInv','Writable','Mask','BitShift','PrintHex','PrintConvColumns','DelValue','AlwaysDecrypt','Prinonv','Priority','DataMember','Groups','SeparateTable','_extra_keys','_shorthand','SubDirectory','Unknown','Protected','List','Avoid','Binary','Hidden','RelatedTag','WriteGroup','Require','Desire','Inhibit','Hook'}
CODE = {
 ('Image::ExifTool::CheckBinaryData','6e141f4f7ef93338d1ccedaa4d66a330f11b5de7695b0789a31fcd87a00521d0'),
 ('Image::ExifTool::WriteBinaryData','6e141f4f7ef93338d1ccedaa4d66a330f11b5de7695b0789a31fcd87a00521d0'),
 ('Image::ExifTool::ProcessBinaryData','c0046e17e1c2640c5aeec98db114c69ebd301fecb049cecdc50a42398b309019'),
 ('Image::ExifTool::ProcessBinaryData','6bcec56a8e09306bf25044e734e138581153361197ad42f96ca8789b4aea6357'),
 # Native 13.59, captured with the canonical Perl after Nikon and
 # NikonCustom hydrate their table graph.  The two historical forms above do
 # not cover this B::Deparse form; it is still closed by this exact body hash.
 ('Image::ExifTool::ProcessBinaryData','283954c79e2a9893469d57fd476091c34b57589f8c8f2e8c44cd62c067738fbf'),
 ('Image::ExifTool::Nikon::ProcessNikonEncrypted','2eaf021035b51e5f8f0577a76420d0d217e3d52d14596fd10c8ff8a53532706b'),
 ('Image::ExifTool::Nikon::ProcessNikonEncrypted','4814b522c2940b28240fc0fbcaa6d22e43619d3e604c3d7a65de4b61513c467e'),
 ('Image::ExifTool::Nikon::Decrypt','b373a90204cb00e317f330a1a8432a75668e027641088a97a7ede89e2c91326d'),
 ('Image::ExifTool::Nikon::Decrypt','5ecb54a37173daf492800e65c341309ce78d56ed7483f6c9efed7bdc4d7e949b'),
 ('Image::ExifTool::Nikon::InitEncryptedSubdir','ceecb4b7085857c703fecfd2fa870ac293f9a2d75b67e8572f26dc82af0df6e8'),
 ('Image::ExifTool::Nikon::InitEncryptedSubdir','56a3cc34bff49394ce5d9e531d762bba5594d6df741150a253f849d8e6415c5e'),
 ('Image::ExifTool::Nikon::PrepareNikonOffsets','437fd2d08043ac213b639560c4a037b4fa0f3950745c07211ae3caa0a381bea5'),
 ('Image::ExifTool::Nikon::PrepareNikonOffsets','0451602b9206b6f48dfce8f69640edba38ef1b51322d6b4fe54b2c7761f79153'),
 ('Image::ExifTool::Nikon::SetByteOrder','ab615336391af90d9ab9b1e146cdcc6fb76bc20e096a3a2afb72d798da5f13d4'),
 ('Image::ExifTool::Nikon::SetByteOrder','b09a10c46f0800e2a2d1bc8cde1269fa205300e62f0d5bf7358d6a57ac2c8d4d'),
 ('Image::ExifTool::SetByteOrder','ab615336391af90d9ab9b1e146cdcc6fb76bc20e096a3a2afb72d798da5f13d4'),
 ('Image::ExifTool::SetByteOrder','b09a10c46f0800e2a2d1bc8cde1269fa205300e62f0d5bf7358d6a57ac2c8d4d'),
}
# The generated root layout delegates decryption to `encrypted.rs`, so this is
# the native callback closure whose behavior the generated runtime actually
# consumes. Keep the helper body pinned, not merely its name or source file:
# an upstream edit to `Decrypt` can otherwise leave ProcessNikonEncrypted's
# body and every table row unchanged while preserving stale Rust semantics.
NIKON_ENCRYPTED_CALLBACKS = {
 ('Image::ExifTool::Nikon::ProcessNikonEncrypted','2eaf021035b51e5f8f0577a76420d0d217e3d52d14596fd10c8ff8a53532706b'): {
  'source_file':'Image/ExifTool/Nikon.pm',
  'dependencies': {
   'Image::ExifTool::Nikon::Decrypt': {
    'body_sha256':'b373a90204cb00e317f330a1a8432a75668e027641088a97a7ede89e2c91326d',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::InitEncryptedSubdir': {
    'body_sha256':'ceecb4b7085857c703fecfd2fa870ac293f9a2d75b67e8572f26dc82af0df6e8',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::PrepareNikonOffsets': {
    'body_sha256':'437fd2d08043ac213b639560c4a037b4fa0f3950745c07211ae3caa0a381bea5',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::SetByteOrder': {
    'name':'Image::ExifTool::SetByteOrder',
    'body_sha256':'ab615336391af90d9ab9b1e146cdcc6fb76bc20e096a3a2afb72d798da5f13d4',
    'source_file':'Image/ExifTool.pm',
   },
 },
},
 # The full table hydration capture uses the canonical Perl 5.38.2
 # B::Deparse spelling.  This remains an exact body-and-closure contract,
 # rather than normalizing deparse text or accepting source provenance alone.
 ('Image::ExifTool::Nikon::ProcessNikonEncrypted','4814b522c2940b28240fc0fbcaa6d22e43619d3e604c3d7a65de4b61513c467e'): {
  'source_file':'Image/ExifTool/Nikon.pm',
  'dependencies': {
   'Image::ExifTool::Nikon::Decrypt': {
    'body_sha256':'5ecb54a37173daf492800e65c341309ce78d56ed7483f6c9efed7bdc4d7e949b',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::InitEncryptedSubdir': {
    'body_sha256':'56a3cc34bff49394ce5d9e531d762bba5594d6df741150a253f849d8e6415c5e',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::PrepareNikonOffsets': {
    'body_sha256':'0451602b9206b6f48dfce8f69640edba38ef1b51322d6b4fe54b2c7761f79153',
    'source_file':'Image/ExifTool/Nikon.pm',
   },
   'Image::ExifTool::Nikon::SetByteOrder': {
    'name':'Image::ExifTool::SetByteOrder',
    'body_sha256':'b09a10c46f0800e2a2d1bc8cde1269fa205300e62f0d5bf7358d6a57ac2c8d4d',
    'source_file':'Image/ExifTool.pm',
   },
  },
 },
}
SPECIAL_PC = {
 '1e7ef329fc2f1932e8487471a4ca9bb29feba8e0c97bd9d10d2328b6bae8c082':'BlockShotBits',
 'fe6312fde652899325bd7fed73115e385b6e3526e114266a2e2e32ccd07007e1':'AutoCaptureCriteriaBits',
 'e39d2bfe1d546432f88f3415aaab130bc72f64ce96e32e4d41b19a0b55d69181':'IntervalShooting',
 'da014b0e5b7c59478c58ec5890bc976d58365078825b704894133e093ed9b9e5':'IntervalShooting',
 'ff1a7de8b648547d7f02aab98716023f6979b176a75e366c99e64f5be1ac37d3':'FocusShiftShooting',
 '6e9bba23a48ab2bf2c5f05e13fbf5e8cd7ffc66c0ed529fc9160be1e0effba94':'LensFirmwareVersion',
 '868d752f06dc5a20eb423df5e6801c3ad9865e9c7b1e13217b4bdc50a31a6137':'FocusDistanceZ',
 'd2f5906504ee7b1abc67b58f6fa82694605b48f0b9be93140222ec6b6590aecd':'FullOrExposureTime',
 '535f8aab6cd7ed2e24f8fecfde9e57227879f01eaf89cbe53326ba9c2fe0c785':'MetersOrInf',
}
OMITTED_PC = {
 'cf84f72cbe6f54d5eb37b21b03f2ff327f792913542050fba29588d3261333af',
 'd038778460baf97908e1d6246b60083b819e3ba8d7605f11a3e15441c81c79df',
 '5934a0489c7e95d2c7a2d733d8b0d6e8f5cd3af2530169a1f8ab54cc6c55c5d2',
}
# `Image/ExifTool/Nikon.pm`, MenuSettingsZ8v2 tag 0 (ExifTool 13.59).  This
# is intentionally an exact source-body allowlist: the runtime hook models
# this one layout adjustment, not arbitrary Perl attached to that table.
MENU_SETTINGS_Z8V2_HOOK = (
 '\n'
 '            if ($$self{FirmwareVersion}) {\n'
 '                if ($$self{FirmwareVersion} =~ /^02\\.10/) {\n'
 '                    $varSize += 4;\n'
 '                } elsif ($$self{FirmwareVersion} ge "03.0") {\n'
 '                    $varSize += 8\n'
 '                }\n'
 '            }\n'
 '        '
)

def fail(x): raise Unsupported(x)
def u(v, bits=32):
 if isinstance(v,bool) or not isinstance(v,(str,int)) or not re.fullmatch(r'0|[1-9][0-9]*',str(v)): fail(f'noncanonical uint{bits}: {v!r}')
 n=int(v)
 if n >= 1<<bits: fail(f'uint{bits} overflow: {v!r}')
 return n
def flag(v, field):
 if v in (True,1,'1'): return 'true'
 if v in (False,0,'0',None): return 'false'
 fail(f'noncanonical {field}: {v!r}')
def low_priority(v):
 if v is None: return 'false'
 if v in (0,'0'): return 'true'
 fail(f'noncanonical Priority: {v!r}')
def rs(v):
 if not isinstance(v,str) or any(0xd800<=ord(c)<=0xdfff for c in v): fail(f'invalid Rust string: {v!r}')
 # Escape source characters, rather than rewriting JSON's escaped output:
 # source ``\\n`` must remain two literal characters, while a source newline
 # must become the Rust newline escape.
 escaped=[]
 for char in v:
  if char=='\\': escaped.append('\\\\')
  elif char=='"': escaped.append('\\"')
  elif char=='\n': escaped.append('\\u{a}')
  elif char=='\r': escaped.append('\\u{d}')
  elif char=='\t': escaped.append('\\u{9}')
  elif ord(char)<0x20 or ord(char)==0x7f: escaped.append(f'\\u{{{ord(char):x}}}')
  else: escaped.append(char)
 return '"'+''.join(escaped)+'"'

def rust_regex(pattern):
 """Admit the shared, structurally validated Perl/Rust regex subset.

 ``conds`` owns the AST grammar used by the runtime condition generator.  It
 rejects general repetition, lookarounds, backreferences and unrecognised
 escapes before a Rust literal exists.  Rust-only character-class set syntax
 has different Perl meaning, so Nikon additionally refuses it here.
 """
 if not isinstance(pattern,str) or any(ord(char)<0x20 or 0xd800<=ord(char)<=0xdfff for char in pattern):
  fail(f'invalid Rust regex: {pattern!r}')
 if re.search(r'\(\?(?:P<|<)',pattern):
  fail(f'capture or lookbehind regex group is outside the generated condition model: {pattern!r}')
 index=0; in_class=False
 while index<len(pattern):
  char=pattern[index]
  if char=='\\':
   if index+1>=len(pattern): fail(f'invalid Rust regex escape: {pattern!r}')
   index+=2; continue
  if char=='[' and not in_class: in_class=True
  elif in_class and (char=='[' or pattern.startswith('&&',index) or pattern.startswith('--',index) or pattern.startswith('~~',index)):
   fail(f'Rust-only character-class set syntax: {pattern!r}')
  elif char==']' and in_class: in_class=False
  index+=1
 if in_class: fail(f'unbalanced Rust regex: {pattern!r}')
 try:
  return conds._validate_regex_pattern(pattern,ascii_source_only=True)
 except conds.CondCompileError as error:
  fail(f'regex outside shared Rust-compatible grammar: {pattern!r}: {error}')
def code(v):
 base={'__perl','__opaque','__name','__deparse'}
 provenance={'resolved','source_file','source_sha256'}
 allowed=base|provenance|{'dependencies','lexical_arrays'}
 if not isinstance(v,dict) or set(v)-allowed or not base <= set(v) or v.get('__perl')!='CODE' or v.get('__opaque') not in (True,1) or not isinstance(v.get('__name'),str) or not isinstance(v.get('__deparse'),str): fail(f'bad CODE: {v!r}')
 present=set(v)&provenance
 if present and (present != provenance or v['resolved'] is not True or not isinstance(v['source_file'],str) or not re.fullmatch(r'[A-Za-z0-9_./-]+',v['source_file']) or v['source_file'].startswith('/') or '..' in v['source_file'].split('/') or not isinstance(v['source_sha256'],str) or not re.fullmatch(r'[0-9a-f]{64}',v['source_sha256'])): fail(f'bad CODE provenance: {v!r}')
 if 'dependencies' in v and (not isinstance(v['dependencies'],dict) or not v['dependencies']): fail(f'bad CODE dependencies: {v!r}')
 pair=(v.get('__name'),hashlib.sha256(v.get('__deparse','').encode()).hexdigest())
 if pair not in CODE: fail(f'unregistered native CODE body: {pair[0]!r} {pair[1]}')
 return pair[0]

def decrypt_xlat(fact):
 """Return the live two-row byte lookup captured from Nikon::Decrypt's pad."""
 xlat=fact.get('lexical_arrays')
 if not isinstance(xlat,dict) or set(xlat)!={'resolved','rows','sha256'} or xlat.get('resolved') is not True:
  fail('Nikon Decrypt: missing or unresolved @xlat closure')
 rows=xlat.get('rows')
 if (not isinstance(rows,list) or len(rows)!=2 or not isinstance(xlat.get('sha256'),str)
     or not re.fullmatch(r'[0-9a-f]{64}',xlat['sha256'])):
  fail('Nikon Decrypt: malformed @xlat closure')
 if any(not isinstance(row,list) or len(row)!=256 for row in rows):
  fail('Nikon Decrypt: @xlat must contain two 256-byte rows')
 if any(isinstance(value,bool) or not isinstance(value,int) or not 0<=value<=255 for row in rows for value in row):
  fail('Nikon Decrypt: @xlat contains non-byte data')
 raw=bytes(rows[0]+rows[1])
 if hashlib.sha256(raw).hexdigest()!=xlat['sha256']:
  fail('Nikon Decrypt: @xlat digest mismatch')
 return raw

def encrypted_callback(v):
 name=code(v); pair=(name,hashlib.sha256(v['__deparse'].encode()).hexdigest())
 expected=NIKON_ENCRYPTED_CALLBACKS.get(pair)
 if expected is None: fail(f'unregistered encrypted callback: {name!r}')
 if v['source_file']!=expected['source_file']:
  fail(f'{name}: source provenance changed')
 deps=v.get('dependencies')
 if not isinstance(deps,dict): fail(f'{name}: missing helper dependencies')
 expected_deps=expected['dependencies']
 if set(deps)!=set(expected_deps): fail(f'{name}: helper graph changed')
 decrypt_lookup=None
 for helper,contract in expected_deps.items():
  fact=deps.get(helper)
  if fact is None: fail(f'{name}: missing helper {helper!r}')
  actual=code(fact)
  digest=hashlib.sha256(fact['__deparse'].encode()).hexdigest()
  if (actual!=contract.get('name',helper) or digest!=contract['body_sha256']
      or fact['source_file']!=contract['source_file']):
   fail(f'{name}: unregistered helper body or source {helper!r} {digest}')
  if fact['source_file']=='Image/ExifTool/Nikon.pm' and fact['source_sha256']!=v['source_sha256']:
   fail(f'{name}: Nikon helper source is not from the callback capture')
  if helper=='Image::ExifTool::Nikon::Decrypt': decrypt_lookup=decrypt_xlat(fact)
 if decrypt_lookup is None: fail(f'{name}: missing Decrypt lookup closure')
 return name,decrypt_lookup
def expr(row,k):
 if k not in row:return None
 v=row[k]
 if not isinstance(v,dict) or set(v)!={'kind','expr'} or not isinstance(v['expr'],str): fail(f'bad {k}: {v!r}')
 return v['expr']
def omitted(row):
 return (expr(row,'RawConv') == 'unless (defined $$self{FocusDistanceRangeWidth} and not $$self{FocusDistanceRangeWidth}) { if ($val == 0 ) {$$self{LensDriveEnd} = "No"} else { $$self{LensDriveEnd} = "CFD"} } else{ $$self{LensDriveEnd} = "Inf"}' or
         (isinstance(row.get('PrintConv'),dict) and row['PrintConv'].get('kind')=='expr' and hashlib.sha256(row['PrintConv']['expr'].encode()).hexdigest() in OMITTED_PC))

def reader_ignored_properties(identity, key, row):
 """Validate native fields with no effect on the generated read interpreter.

 ``DelValue`` drives ExifTool writes only. ``AlwaysDecrypt`` controls native
 pre-decryption directory-length discovery; this runtime decrypts the complete
 buffer before calling ``process``. ``Prinonv`` is a one-off misspelled native
 property and has no Image::ExifTool consumer. Keep each captured value, and
 reject new placements or shapes instead of treating arbitrary extra fields as
 reader-inert.
 """
 if 'DelValue' in row:
  u(row['DelValue'])
 if 'AlwaysDecrypt' in row and row['AlwaysDecrypt'] not in (1, True):
  fail(f'{identity}[{key}]: unsupported AlwaysDecrypt {row["AlwaysDecrypt"]!r}')
 if 'Prinonv' in row:
  expected = {'0':'None','1':'Choose Image Area','2':'One Step Speed/Aperture',
              '3':'Choose Non-CPU Lens Number','5':'Auto bracketing',
              '6':'Dynamic AF Area','7':'Shutter speed & Aperture lock'}
  if identity != ('NikonCustom','SettingsD700') or key != '32.1' or row['Prinonv'] != expected:
   fail(f'{identity}[{key}]: unregistered reader-ignored Prinonv')
def dm(s):
 if s not in DMS: fail(f'unknown data member {s!r}')
 return f'Dm::{s}'
def num(x):
 # Native numeric literals here are integral; write the exact f64 spelling used by the checked table.
 return f'{float(x):.1f}'

def cond(s):
 if s is None:return 'Cond::Always'
 if s in ('$$self{FlashGroupOptionsMasterMode}  != 3','$$self{FlashGroupOptionsMasterMode}  == 1','$$self{MovieType}  != 1'):
  member,op,value=re.fullmatch(r'\$\$self\{(\w+)\}\s+(==|!=) (\d+)',s).groups()
  return f'Cond::Num({dm(member)}, NumCmp::{"Eq" if op=="==" else "Ne"}, {num(value)})'
 m=re.fullmatch(r'\$\$self\{Model\} ([=!])~ /(.+)/([i]?)',s)
 if m:return f'Cond::Model({str(m.group(1)=="=").lower()}, {rs(rust_regex(m.group(2)))}, {str(bool(m.group(3))).lower()})'
 m=re.fullmatch(r'\$\$self\{FILE_TYPE\} (eq|ne) "([^"]+)"',s)
 if m:return f'Cond::FileType({str(m.group(1)=="eq").lower()}, {rs(m.group(2))})'
 m=re.fullmatch(r'\$\$self\{(\w+)\} (==|!=|>=|>|<) (-?\d+)',s)
 if m:return f'Cond::Num({dm(m.group(1))}, NumCmp::{ {"==":"Eq","!=":"Ne",">=":"Ge",">":"Gt","<":"Lt"}[m.group(2)] }, {num(m.group(3))})'
 if s=='$$self{HDR} ne 0': return 'Cond::StrEq(Dm::HDR, "0", false)'
 m=re.fullmatch(r'\$\$self\{(\w+)\} and \$\$self\{\1\} ne (\d+)',s)
 if m:return f'Cond::TruthyStrNe({dm(m.group(1))}, {rs(m.group(2))})'
 m=re.fullmatch(r'\$\$self\{(\w+)\} and \$\$self\{\1\} (==|!=|>=|>|<) (-?\d+)',s)
 if m:return f'Cond::TruthyNum({dm(m.group(1))}, NumCmp::{ {"==":"Eq","!=":"Ne",">=":"Ge",">":"Gt","<":"Lt"}[m.group(2)] }, {num(m.group(3))})'
 m=re.fullmatch(r'\$\$self\{(\w+)\}',s)
 if m:return f'Cond::Truthy({dm(m.group(1))})'
 m=re.fullmatch(r'\$\$self\{(\w+)\} and \$\$self\{\1\} ne "([^"]+)"',s)
 if m:return f'Cond::TruthyStrNe({dm(m.group(1))}, {rs(m.group(2))})'
 m=re.fullmatch(r'\$\$self\{FirmwareVersion\} and \$\$self\{FirmwareVersion\} (ge|lt) "([^"]+)"',s)
 if m:return f'Cond::FirmwareCmp(StrCmp::{"Ge" if m.group(1)=="ge" else "Lt"}, {rs(m.group(2))}, true)'
 m=re.fullmatch(r'\$\$self\{FirmwareVersion\} (ge|eq|!~|=~) (?:"([^"]+)"|/(.+)/)',s)
 if m:
  if m.group(1)=='ge':return f'Cond::FirmwareCmp(StrCmp::Ge, {rs(m.group(2))}, false)'
  if m.group(1)=='eq':return f'Cond::FirmwareEq({rs(m.group(2))}, true)'
  return f'Cond::FirmwareRe({rs(rust_regex(m.group(3)))}, {str(m.group(1)=="=~").lower()})'
 m=re.fullmatch(r'\$\$self\{ShotInfoVersion\} eq "([^"]+)"',s)
 if m:return f'Cond::StrEq(Dm::ShotInfoVersion, {rs(m.group(1))}, true)'
 if s=='$$self{LensID} and $$self{LensID} != 0 and $$self{FocusMode} ne "Manual"':return 'Cond::TruthyNumAndStrNe(Dm::LensID, NumCmp::Ne, 0.0, Dm::FocusMode, "Manual")'
 if s=='$$self{ShutterMode} and $$self{ShutterMode} ne 96 and $$self{FocusShiftShooting} > 0':return 'Cond::TruthyStrNeAndNum(Dm::ShutterMode, "96", Dm::FocusShiftShooting, NumCmp::Gt, 0.0)'
 if s=='$$self{ShutterMode} and $$self{ShutterMode} ne 96 and $$self{IntervalShooting} > 0':return 'Cond::TruthyStrNeAndNum(Dm::ShutterMode, "96", Dm::IntervalShooting, NumCmp::Gt, 0.0)'
 # These two have distinct runtime forms and are kept literal to avoid a Perl grammar.
 literal={'$$self{Model} eq "NIKON D5" and $$self{FirmwareVersion} ge "1.40"':'Cond::ModelEqAndFirmwareGe("NIKON D5", "1.40")', '$$self{Model} =~ /^NIKON Z6_3\\b/i and $$self{FirmwareVersion} and $$self{FirmwareVersion} lt "02.00"':f'Cond::ModelAndFirmware(true, {rs("^NIKON Z6_3\\b")}, true, StrCmp::Lt, "02.00")'}
 if s in literal:return literal[s]
 fail(f'unregistered Condition: {s!r}')

def raw(s):
 if s is None:return 'Raw::None','Filter::None'
 m=re.fullmatch(r'\$\$self\{(\w+)\} = \$val',s)
 if m:return f'Raw::Store({dm(m.group(1))})','Filter::None'
 exact={'$val = $val/256':('Raw::Div256','Filter::None'),'$val || undef':('Raw::NonZero','Filter::None'),'$val =~ /^\\d\\.\\d+.$/ ? $val : undef':('Raw::FirmwareLike','Filter::None'),'$$self{OldLensData} = 1 unless $val =~ /^.\\0+$/s; undef':('Raw::StoreFlagIfNotPadding(Dm::OldLensData)','Filter::None'),'$$self{NewLensData} = 1 unless $val =~ /^.\\0+$/s; undef':('Raw::StoreFlagIfNotPadding(Dm::NewLensData)','Filter::None'),'$$self{ShotInfoVersion} = $val; $val =~ /^\\d+$/ ? $val : undef':('Raw::Store(Dm::ShotInfoVersion)','Filter::DigitsOnly'),'$$self{AFAreaInitialWidth} = 1 + int ($val / 4)':('Raw::StoreScaled(Dm::AfAreaInitialWidth, 4.0)','Filter::None'),'$$self{AFAreaInitialHeight} = 1 + int ($val / 7) ':('Raw::StoreScaled(Dm::AfAreaInitialHeight, 7.0)','Filter::None')}
 if s in exact:return exact[s]
 fail(f'unregistered RawConv: {s!r}')

def vc(s):
 if s is None:return 'Vc::None'
 exact={'($val > 0x7 ? $val - 0x10 : $val) / 6':'Vc::Nibble4Div6','100*exp(($val/12-5)*log(2))':'Vc::Iso100Exp','0.01 * 10**($val/40)':'Vc::Pow10Div40','$val ? 2048 / $val : $val':'Vc::Recip2048','$val <= 180 ? $val : $val - 360':'Vc::Signed360','($val eq -1 ?  \'No Limit\' : $val ) ':'Vc::NoLimit','my $t = ($val - 16) % 24; $t ? $val / 24  : 2 + ($val - 16) / 24':'Vc::D3bShutterSpeed','$$self{SingleFrame} == 0 ? 5 : $val':'Vc::SingleFrameOrFive','unpack("n", $val)':'Vc::UnpackBigEndian16','5 * 2**($val/24)':'Vc::FiveTimesPow2Div24','2**($val/384-1)':'Vc::Pow2Div384Minus1','2**(($val-80)/12)':'Vc::Pow2SubDiv(80.0, 12.0)','$val < 10 ? $val + 1 : 5 * ($val - 7)':'Vc::SmallOrScaled(5.0, 7.0)','$val < 10 ? $val + 1 : 10 * ($val - 8)':'Vc::SmallOrScaled(10.0, 8.0)','-$val/6':'Vc::NegDiv(6.0)','$val - 5':'Vc::Add(-5.0)','2 ** (-$val)':'Vc::Pow2NegSub(0.0)'}
 if s in exact:return exact[s]
 for pat,out in [(r'2\*\*\(\$val/(\d+)\)','Vc::Pow2Div({})'),(r'2 \*\* \(-\$val/(\d+)\)','Vc::Pow2NegDiv({})'),(r'2 \*\* \(-\$val-(\d+)\)','Vc::Pow2NegSub({})'),(r'2 \*\* \(\$val - (\d+)\)','Vc::Pow2Sub({})'),(r'\$val / (\d+)','Vc::Div({})'),(r'\$val/(\d+)','Vc::Div({})'),(r'\$val \+ (\d+)','Vc::Add({})'),(r'\(\$val-(\d+)\)/(\d+)','Vc::SubDiv({}, {})'),(r'\(\$val - (\d+)\) / (\d+)','Vc::SubDiv({}, {})')]:
  m=re.fullmatch(pat,s)
  if m:return out.format(*[num(x) for x in m.groups()])
 fail(f'unregistered ValueConv: {s!r}')

def pc(row, maps):
 v=row.get('PrintConv')
 if v is None:return 'Pc::None'
 if isinstance(v,dict) and v.get('kind') in ('enum','enum_partial'):
  if set(v)-{'kind','map','directives'}:fail('bad enum fields')
  mp=v.get('map'); ds=v.get('directives')
  if not isinstance(mp,dict):fail('bad enum map')
  pairs=tuple(sorted(mp.items()))
  for a,b in pairs:rs(a);rs(b)
  if ds is None:
   i=maps.setdefault(('map',pairs),len(maps));return f'Pc::Map(M{i})'
  if not isinstance(ds,dict) or set(ds)!={'BITMASK'} or not isinstance(ds['BITMASK'],dict):fail('bad BITMASK')
  bits=tuple(sorted((u(k,5),v) for k,v in ds['BITMASK'].items()))
  for _,x in bits:rs(x)
  i=maps.setdefault(('bit',pairs,bits),len(maps));return f'Pc::Bitmask(M{i}, B{i})'
 s=expr(row,'PrintConv'); h=hashlib.sha256(s.encode()).hexdigest()
 if h in SPECIAL_PC:return 'Pc::'+SPECIAL_PC[h]
 exact={'"$val fps"':'Pc::Suffix(" fps")','"$val Hz"':'Pc::Suffix(" Hz")','"$val mm"':'Pc::Suffix(" mm")','"+/-$val"':'Pc::Prefix("+/-")','Image::ExifTool::Exif::PrintExposureTime($val)':'Pc::ExposureTime','Image::ExifTool::Exif::PrintFraction($val)':'Pc::Fraction','int($val + 0.5)':'Pc::RoundHalfUp','sprintf("0x%02x", $val)':'Pc::Hex2','$val == 1? "1 Second" : sprintf("%.0f Seconds",$val)':'Pc::Seconds','$val>0.99 ? "Full" : sprintf("%.1f%%",$val*100)':'Pc::FullOrPercent','$val == 0? "No Delay" : sprintf("%.0f sec",$val)':'Pc::NoDelayOrSeconds','$val ? sprintf("%.1f sec",$val/1000) : "Off"':'Pc::SecondsDiv1000OrOff','$val > 0 ? sprintf("%.0f", $val) : ""':'Pc::PositiveOrBlank'}
 if s in exact:return exact[s]
 pats=[(r'sprintf\("f/%\.1f",\$val/100\)','Pc::FNumberDiv100'),(r'sprintf\("%\.1fmm",\$val/(\d+)\)','Pc::MmDiv({})'),(r'sprintf\("%\.1f mm",\$val\)','Pc::FixedSuffix(1, " mm")'),(r'sprintf\("%\.1f m", \$val/10\)','Pc::MetersDiv10'),(r'\$val \? sprintf\("%\+\.(\d)f", ?\$val\) : 0','Pc::SignedOrZero({})'),(r'sprintf\("%\+\.(\d)f",\$val\)','Pc::Signed({})'),(r'sprintf\("%\.(\d)f", ?\$val\)','Pc::Fixed({})')]
 for pat,out in pats:
  m=re.fullmatch(pat,s)
  if m:
   if out == 'Pc::MmDiv({})': return out.format(num(m.group(1)))
   return out.format(m.group(1)) if '{}' in out else out
 fail(f'unregistered PrintConv: {s!r}')

HEADER='''//! Nikon encrypted binary-data tables -- generated, do not hand-edit.\n//!\n//! Every row below was read out of ExifTool's own `%Image::ExifTool::Nikon::*`\n//! and `%Image::ExifTool::NikonCustom::*` hashes in-process (ExifTool 13.59),\n//! so the offsets, masks, formats, Conditions and PrintConv tables are\n//! ExifTool's rather than retyped. A tag whose Condition or conversion is not\n//! one of the forms [`super::binary_data`] implements is omitted entirely\n//! rather than emitted with a guessed value.\n\nuse super::binary_data::{\n    BinTable, BinTag, Cond, Dm, Encrypted, Filter, Fmt, Hook, NumCmp, Pc, Raw, Root, StrCmp,\n    SubDir, SubStart, Vc,\n};\n'''

def render(data):
 if data.get('exiftool_version')!='13.59':fail('wrong ExifTool version')
 mods=data.get('modules',{}); nik=mods.get('Nikon',{}).get('tables',{}); custom=mods.get('NikonCustom',{}).get('tables',{})
 if not nik or not custom:fail('missing Nikon modules')
 def table(full):
  m=re.fullmatch(r'Image::ExifTool::(Nikon|NikonCustom)::(\w+)',full or '')
  if not m:fail(f'bad TagTable {full!r}')
  identity=(m.group(1),m.group(2)); t=(nik if identity[0]=='Nikon' else custom).get(identity[1])
  if not t:fail(f'missing native table {full}')
  return identity,t
 def selected(identity):
  return (nik if identity[0]=='Nikon' else custom)[identity[1]]
 def static_name(identity):
  # Keep established identifiers byte-for-byte when a table name is unique.
  # Namespace it only when the reachable graph contains a real collision.
  if sum(name==identity[1] for _,name in names)==1:
   return identity[1].upper()
  return '_'.join(identity).upper()
 roots=[]; queue=[]; decrypt_lookup=None
 for tag,which in [('145','SHOT_INFO_ROOTS'),('151','COLOR_BALANCE_ROOTS'),('152','LENS_DATA_ROOTS')]:
  group=nik.get('Main',{}).get('tags',{}).get(tag)
  if not group:fail(f'missing Nikon::Main[{tag}]')
  variants=group.get('_variants',[group]); out=[]
  for row in variants:
   sd=row.get('SubDirectory'); name=row.get('Name'); c=row.get('Condition')
   if not isinstance(sd,dict) or not isinstance(name,str):fail('bad root variant')
   if c is None: c='$$valPt =~ /^/'
   if not isinstance(c,str):fail('bad root condition')
   m=re.fullmatch(r'\$\$valPt =~ /\^(.*)/(?:(?: and )(.+))?',c)
   if not m:fail(f'unsupported root condition {c!r}')
   ver=m.group(1); guard=m.group(2); cap='None';counts='&[]'
   if guard:
    q=re.fullmatch(r'\$1 < (\d+)',guard)
    if q:cap=f'Some({u(q.group(1))})'
    elif q:=re.fullmatch(r'\$count == (\d+)',guard):
     counts='&['+str(u(q.group(1)))+']'
    elif re.fullmatch(r'\(\$count == \d+(?: or \$count == \d+)+\)',guard):
     vals=[u(part.removeprefix('$count == ')) for part in guard[1:-1].split(' or ')]
     counts='&['+', '.join(str(value) for value in vals)+']'
    else:fail(f'unsupported root guard {guard!r}')
   full=sd.get('TagTable'); tname,t=table(full)
   encrypted=None
   if 'ProcessProc' in sd or 'DecryptStart' in sd:
    if 'ProcessProc' in sd:
     callback,lookup=encrypted_callback(sd['ProcessProc'])
     if callback!='Image::ExifTool::Nikon::ProcessNikonEncrypted':fail('unexpected root process')
     if decrypt_lookup is None: decrypt_lookup=lookup
     elif decrypt_lookup!=lookup: fail('Nikon roots disagree about Decrypt @xlat')
    queue.append(tname)
    order=sd.get('ByteOrder'); bo={'BigEndian':'Some(true)','LittleEndian':'Some(false)',None:'None'}.get(order)
    if bo is None:fail(f'bad root ByteOrder {order!r}')
    encrypted=f'Some(Encrypted {{ table: {{TABLE:{tname[0]}:{tname[1]}}}, decrypt_start: {u(sd.get("DecryptStart",0))}, dir_offset: {u(sd.get("DirOffset",0))}, byte_order: {bo} }})'
   out.append((name,ver,cap,counts,encrypted))
  roots.append((tag,which,out))
 seen=set()
 while queue:
  n=queue.pop()
  if n in seen:continue
  seen.add(n); t=selected(n)
  for _,g in sorted(t.get('tags',{}).items(),key=lambda x: float(x[0])):
   for row in g.get('_variants',[g]):
    sd=row.get('SubDirectory')
    if sd:
     child,_=table(sd.get('TagTable')); queue.append(child)
 names=sorted(seen); idx={n:i for i,n in enumerate(names)}; maps={}; rows={}
 # Enum declaration order follows the loaded native hashes.  Tags themselves
 # are emitted in ProcessBinaryData numeric order below.
 for n in names:
  t=selected(n)
  for _,g in sorted(t.get('tags',{}).items(),key=lambda x: float(x[0])):
   for row in g.get('_variants',[g]):
    if not omitted(row) and 'PrintConv' in row: pc(row,maps)
 for n in names:
  t=selected(n); meta=t.get('meta',{})
  for k in ('CHECK_PROC','PROCESS_PROC','WRITE_PROC'):
   if k in meta:code(meta[k])
  proc=meta.get('PROCESS_PROC')
  if proc is not None and proc.get('__name') not in ('Image::ExifTool::ProcessBinaryData','Image::ExifTool::Nikon::ProcessNikonEncrypted'):fail(f'{n}: bad PROCESS_PROC')
  if set(meta)-{'CHECK_PROC','PROCESS_PROC','WRITE_PROC','WRITABLE','FIRST_ENTRY','GROUPS','NOTES','FORMAT','DATAMEMBER','IS_SUBDIR','VARS'}:fail(f'{n}: unsupported meta')
  out=[]
  for key,g in sorted(t.get('tags',{}).items(),key=lambda x:float(x[0])):
   m=re.fullmatch(r'(-?\d+)(?:\.(\d+))?',key)
   if not m:fail(f'{n}: invalid index {key!r}')
   for row in g.get('_variants',[g]):
    if set(row)-ALLOWED:fail(f'{n}[{key}]: unsupported fields {set(row)-ALLOWED}')
    if row.get('_extra_keys',[]) != []:fail(f'{n}[{key}]: unrecognized dumped fields {row.get("_extra_keys")!r}')
    # ExifTool uses this only to arrange its human-readable PrintConv list.
    # Scalar tag lookup applies the same map regardless of the display width,
    # which the Rust metadata API does not expose.  Still validate the native
    # shape so an executable or malformed replacement cannot disappear here.
    if 'PrintConvColumns' in row and (isinstance(row['PrintConvColumns'],bool) or not isinstance(row['PrintConvColumns'],int) or row['PrintConvColumns'] < 1):
     fail(f'{n}[{key}]: invalid PrintConvColumns {row["PrintConvColumns"]!r}')
    reader_ignored_properties(n, key, row)
    # These declarations need runtime operations absent from binary_data.rs.
    # They are intentionally omitted by the checked-in projection; every
    # other unregistered executable fact remains a hard refusal.
    if omitted(row):
     continue
    name=row.get('Name');
    if not isinstance(name,str) or not name:fail(f'{n}[{key}]: bad Name')
    fmt=row.get('Format'); count=1
    if fmt is None:rf='Default'
    else:
     z=re.fullmatch(r'(\w+)(?:\[(\d+)\])?',fmt or '')
     if not z or z.group(1) not in FORMATS:fail(f'{n}[{key}]: bad Format {fmt!r}')
     rf=FORMATS[z.group(1)];count=u(z.group(2) or 1)
    mask=u(row.get('Mask',0)); shift=(mask & -mask).bit_length()-1 if mask else 0
    if 'BitShift' in row and u(row['BitShift'])!=shift:fail(f'{n}[{key}]: explicit BitShift')
    sd=row.get('SubDirectory'); sub='None'
    if sd is not None:
     if not isinstance(sd,dict) or set(sd)-{'TagTable','Start'}:fail(f'{n}[{key}]: bad SubDirectory')
     child,_=table(sd.get('TagTable')); start={'$val':'Val','$dirStart + $val':'DirStartPlusVal'}.get(sd.get('Start'),'Fixed(0)' if sd.get('Start') is None else None)
     if start is None:fail(f'{n}[{key}]: bad SubDirectory Start')
     sub=f'Some(SubDir {{ table: {idx[child]}, start: SubStart::{start} }})'
    hook='Hook::None'
    h=row.get('Hook')
    if h is not None:
     q=re.fullmatch(r'\$varSize \+= (\d+) if \$\$self\{FirmwareVersion\} and \$\$self\{FirmwareVersion\} ge "([^"]+)"',h)
     if q:hook=f'Hook::AddIfFirmwareGe({u(q.group(1))}, {rs(q.group(2))})'
     elif (q:=re.fullmatch(r'\$varSize \+= (\d+) if \$\$self\{Model\} =~ /(.+)/ and \$\$self\{FirmwareVersion\} and \$\$self\{FirmwareVersion\} ge "([^"]+)"',h)):hook=f'Hook::AddIfModelAndFirmwareGe({u(q.group(1))}, {rs(rust_regex(q.group(2)))}, {rs(q.group(3))})'
     elif n==('Nikon','MenuSettingsZ8v2') and key=='0' and h==MENU_SETTINGS_Z8V2_HOOK:hook='Hook::MenuSettingsZ8v2'
     else:fail(f'{n}[{key}]: unsupported Hook')
    rawv,filterv=raw(expr(row,'RawConv')); unknown=flag(row.get('Unknown',False),'Unknown'); low=low_priority(row.get('Priority'))
    out.append(f'    BinTag {{ index: {m.group(1)}, frac: {m.group(2) or 0}, name: {rs(name)}, cond: {cond(row.get("Condition"))}, fmt: Fmt::{rf}, count: {count}, mask: 0x{mask:x}, shift: {shift}, raw: {rawv}, filter: {filterv}, vc: {vc(expr(row,"ValueConv"))}, pc: {pc(row,maps)}, hook: {hook}, print_hex: {flag(row.get("PrintHex",False),"PrintHex")}, unknown: {unknown}, low_priority: {low}, subdir: {sub} }},')
  rows[n]=out
 # Root tables refer to numeric indices after the graph is fixed.
 text=[HEADER]
 if decrypt_lookup is not None:
  for name,offset in (('XLAT0',0),('XLAT1',256)):
   values=', '.join(f'0x{value:02x}' for value in decrypt_lookup[offset:offset+256])
   text += ['#[rustfmt::skip]',f'pub static {name}: [u8; 256] = [{values}];']
 for key,i in maps.items():
  if key[0]=='map':
   pairs=key[1]; body=', '.join('('+rs(a)+', '+rs(b)+')' for a,b in pairs)
   text += ['#[rustfmt::skip]',f'static M{i}: &[(&str, &str)] = &[{body}{"," if body else ""}];']
  else:
   pairs,bits=key[1],key[2]; body=', '.join('('+rs(a)+', '+rs(b)+')' for a,b in pairs)
   text += ['#[rustfmt::skip]',f'static M{i}: &[(&str, &str)] = &[{body}{"," if body else ""}];','#[rustfmt::skip]',f'static B{i}: &[(u32, &str)] = &[{", ".join("("+str(a)+", "+rs(b)+")" for a,b in bits)},];']
 text.append('')
 for n in names:text += ['#[rustfmt::skip]',f'static TAGS_{static_name(n)}: &[BinTag] = &[',*rows[n],'];']
 text += ['', '/// Every reachable encrypted table, indexed by [`SubDir::table`].','#[rustfmt::skip]','pub static TABLES: &[BinTable] = &[']
 for n in names:
  t=selected(n);fmt=t['meta'].get('FORMAT','int8u'); inc={'int8u':1,'int16u':2}.get(fmt)
  if inc is None:fail(f'{n}: unsupported table FORMAT')
  no=t['meta'].get('VARS',{}).get('NIKON_OFFSETS'); no='None' if no is None else f'Some({u(no)})'
  text.append(f'    BinTable {{ name: {rs(n[1])}, increment: {inc}, nikon_offsets: {no}, tags: TAGS_{static_name(n)} }},')
 text.append('];')
 for tag,which,out in roots:
  text += ['',f'/// `Nikon::Main` 0x{int(tag):04x}, in ExifTool\'s Condition order.','#[rustfmt::skip]',f'pub static {which}: &[Root] = &[']
  for name,ver,cap,counts,e in out:
   en='None' if e is None else re.sub(r'\{TABLE:(Nikon|NikonCustom):(\w+)\}',lambda m:str(idx[(m.group(1),m.group(2))]),e)
   text.append(f'    Root {{ name: {rs(name)}, version_re: {rs("^"+ver)}, cap_lt: {cap}, counts: {counts}, encrypted: {en} }},')
  text.append('];')
 return '\n'.join(text)+'\n',{'tables':len(names),'rows':sum(len(x) for x in rows.values()),'maps':len(maps)}

def main():
 p=argparse.ArgumentParser();p.add_argument('dump',type=Path);p.add_argument('-o','--output',required=True,type=Path);a=p.parse_args()
 try:
  data=json.loads(a.dump.read_text()); text,counts=render(data); a.output.write_bytes(text.encode()); print('gen_nikon_encrypted_tables: '+json.dumps(counts,sort_keys=True),file=sys.stderr)
 except (OSError,ValueError,KeyError,TypeError) as e:p.exit(1,f'gen_nikon_encrypted_tables: {e}\n')
if __name__=='__main__':main()
