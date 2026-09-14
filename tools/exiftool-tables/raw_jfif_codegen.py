"""Compile native JFIF DATAMEMBER inputs for generic raw-property extraction.

Only unedited scalar unsigned fields and default native options are supported.
Every recorded dispatcher/reader body is consumed by a finite source grammar;
new executable statements are refused even when an expected branch survives.
"""
from __future__ import annotations
from dataclasses import asdict, dataclass
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
from typing import Any, Mapping

HERE = Path(__file__).resolve().parent

class Refused(ValueError): pass

@dataclass(frozen=True)
class Field:
    property: str
    offset: int
    width: int

@dataclass(frozen=True)
class Recipe:
    marker: int
    signature_hex: str
    skip: int
    byte_order: str
    fields: tuple[Field, ...]


def mapping(value: Any, label: str) -> Mapping:
    if not isinstance(value, Mapping): raise Refused(f'{label} is unavailable')
    return value


def source(fact: Mapping, closure: Mapping, label: str) -> None:
    provenance=mapping(fact.get('source'), f'{label} provenance')
    file=provenance.get('file'); sha=provenance.get('sha256')
    if (file not in {'Image/ExifTool.pm','Image/ExifTool/Writer.pl'}
            or not isinstance(sha,str) or re.fullmatch('[0-9a-f]{64}',sha) is None
            or closure.get(file)!=sha): raise Refused(f'{label} does not join selected native source')


def function(fact: Any, name: str, closure: Mapping) -> Mapping:
    fact=mapping(fact,name)
    if fact.get('resolved') is not True or fact.get('name')!='Image::ExifTool::'+name:
        raise Refused(f'{name} is unresolved or rebound')
    source(fact,closure,name)
    return fact


def compile_fact(fact: Any) -> Recipe:
    fact=mapping(fact,'raw JFIF capture')
    if fact.get('schema')!=1 or fact.get('kind')!='raw_jfif_native_fact': raise Refused('unsupported raw JFIF capture')
    closure=mapping(fact.get('loaded_closure'),'native closure')
    funcs=mapping(fact.get('functions'),'native functions')
    contract=json.loads((HERE/'raw_jfif_source_grammar.json').read_text())
    grammar=contract['functions']
    substitutions={
        '@@MARKER@@':r'(?P<marker>[0-9]+)',
        '@@SIGNATURE@@':r'(?P<signature>[A-Za-z0-9]+)',
        '@@SIGNATURE_AGAIN@@':r'(?P=signature)',
        '@@ORDER@@':r'(?P<order>MM|II)',
        '@@START@@':r'(?P<skip>[0-9]+)',
        '@@START_AGAIN@@':r'(?P=skip)',
    }
    dispatch=None
    if set(funcs)!=set(grammar): raise Refused('native source function inventory is unsupported')
    for name, lines in grammar.items():
        expected="\n".join(lines)
        native=function(funcs.get(name),name,closure)
        if native.get('requested_binding')!='Image::ExifTool::'+name: raise Refused('native requested binding is stale')
        body=native.get('body')
        if not isinstance(body,str): raise Refused('native executable body unavailable')
        if name=='WriteJPEG':
            pattern=re.escape(expected)
            for token,replacement in substitutions.items(): pattern=pattern.replace(re.escape(token),replacement)
            dispatch=re.fullmatch(pattern,body)
            if dispatch is None: raise Refused('WriteJPEG is outside the complete raw JFIF dispatcher grammar')
        elif body != expected:
            raise Refused(f'{name} is outside the complete raw JFIF reader grammar')
    assert dispatch is not None
    marker,skip=int(dispatch['marker']),int(dispatch['skip'])
    prescan=contract['prescan_dispatch_join']
    if marker!=prescan['marker'] or dispatch['signature']!=prescan['signature']:
        raise Refused('raw JFIF dispatcher does not join the complete native pre-scan grammar')
    if not 0<=marker<=255 or not 0<=skip<=65535: raise Refused('raw JFIF dispatcher operands exceed supported range')

    reader=mapping(funcs['ReadValue'].get('lexical_hashes'),'ReadValue lexical hashes')
    processors=mapping(funcs['ProcessBinaryData'].get('lexical_hashes'),'ProcessBinaryData lexical hashes')
    sizes=mapping(reader.get('%formatSize'),'ReadValue format sizes')
    processor_sizes=mapping(processors.get('%formatSize'),'binary processor format sizes')
    dispatch_entries=mapping(reader.get('%readValueProc'),'ReadValue dispatch entries')
    rationals=mapping(reader.get('%isRational'),'ReadValue rational controls')
    for format_name,primitive,width in [('int8u','Get8u',1),('int16u','Get16u',2)]:
        if type(sizes.get(format_name)) is not int or sizes[format_name]!=width or processor_sizes.get(format_name)!=width or rationals.get(format_name):
            raise Refused('raw JFIF unsigned format sizing/dispatch is unsupported')
        entry=mapping(mapping(dispatch_entries.get(format_name),'native primitive').get('code'),'native primitive code')
        function(entry,primitive,closure)
        if entry.get('body')!=funcs[primitive]['body'] or entry.get('source')!=funcs[primitive]['source']:
            raise Refused('raw JFIF primitive does not join captured native reader')
    byte_maps=mapping(funcs['SetByteOrder'].get('lexical_hashes'),'byte-order lexical maps')
    for name,expected in [('%unpackMotorola',{'C':'C','S':'n'}),('%unpackIntel',{'C':'C','S':'v'})]:
        captured=mapping(byte_maps.get(name),name)
        if any(captured.get(k)!=v for k,v in expected.items()): raise Refused('native byte-order templates are unsupported')

    table=mapping(fact.get('table'),'raw JFIF table');source(table,closure,'JFIF table')
    if table.get('binding')!='Image::ExifTool::JFIF::Main': raise Refused('raw JFIF table binding differs from dispatcher')
    entries=mapping(table.get('entries'),'raw JFIF table entries')
    if entries.get('GROUPS')!={'0':'JFIF','1':'JFIF','2':'Image'}:
        raise Refused('raw JFIF native directory groups are unsupported')
    allowed_meta={'PROCESS_PROC','WRITE_PROC','CHECK_PROC','GROUPS','DATAMEMBER','FORMAT'}
    if any(key not in allowed_meta and re.fullmatch(r'[0-9]+',key) is None for key in entries):
        raise Refused('raw JFIF table has unknown control metadata')
    for control,name in [('PROCESS_PROC','ProcessBinaryData'),('WRITE_PROC','WriteBinaryData')]:
        proc=mapping(mapping(entries.get(control),control).get('code'),control+' code')
        function(proc,name,closure)
        if proc.get('body')!=funcs[name]['body'] or proc.get('source')!=funcs[name]['source']:
            raise Refused('raw JFIF table procedure differs from the modeled native binding')
    # The fully consumed ProcessBinaryData production chooses the literal
    # int8u when FORMAT is absent; the captured size map supplies its stride.
    default=entries.get('FORMAT') or 'int8u'
    if default not in {'int8u','int16u'}: raise Refused('raw JFIF default format is unsupported')
    stride=sizes[default]
    ids=entries.get('DATAMEMBER')
    if not isinstance(ids,list) or any(type(i) is not int or i<0 or i>65535 for i in ids):
        raise Refused('raw JFIF DATAMEMBER order is unavailable')
    fields=[]
    # Raw assignments are independent scalar outputs in this specialization.
    # A renamed destination that aliases a native reader/writer control member
    # (for example OPTIONS) changes execution, rather than naming an output.
    reserved_properties=set(re.findall(r"->\{'([A-Za-z_]\w*)'\}",
        '\n'.join('\n'.join(lines) for lines in grammar.values())))
    allowed_row={'Name','Format','Writable','RawConv','PrintConv','Priority','Mandatory','Notes','Description'}
    for index in ids:
        row=mapping(entries.get(str(index)),f'raw JFIF member {index}')
        if set(row)-allowed_row: raise Refused('raw JFIF member has unsupported conversion/control')
        raw=row.get('RawConv')
        match=re.fullmatch(r'\$\$self\{([A-Za-z_]\w*)\}\s*=\s*\$val',raw) if isinstance(raw,str) else None
        if not match: raise Refused('raw JFIF assignment is outside the complete conversion grammar')
        if match[1] in reserved_properties: raise Refused('raw JFIF assignment aliases a native control property')
        fmt=row.get('Format') or default
        if fmt not in {'int8u','int16u'}: raise Refused('raw JFIF member format is unsupported')
        fields.append(Field(match[1],index*stride,sizes[fmt]))
    return Recipe(marker,(dispatch['signature'].encode('ascii')+b'\0').hex(),skip,dispatch['order'],tuple(fields))


def evaluate(recipe: Recipe, marker: int, payload: bytes) -> dict[str,int] | None:
    if marker!=recipe.marker or not payload.startswith(bytes.fromhex(recipe.signature_hex)): return None
    raw=payload[recipe.skip:]; result={}
    for field in recipe.fields:
        if field.offset>=len(raw): break  # native ProcessBinaryData ends this ordered DATAMEMBER pass
        value=raw[field.offset:field.offset+field.width]
        if len(value)!=field.width: continue  # native ReadValue has no complete scalar
        result[field.property]=int.from_bytes(value,'big' if recipe.byte_order=='MM' else 'little')
    return result


def generate(fact: Any, selected_perl: str | None = None) -> tuple[str,dict]:
    if selected_perl is not None:
        identity=mapping(fact.get('native_identity'),'native identity')
        if Path(shutil.which(selected_perl) or selected_perl).resolve()!=Path(identity['perl']).resolve(): raise Refused('raw JFIF capture Perl differs from selected interpreter')
    recipe=compile_fact(fact)
    signature=', '.join(str(byte) for byte in bytes.fromhex(recipe.signature_hex))
    rows='\n'.join(f'    RawField {{ property: {json.dumps(f.property)}, offset: {f.offset}, width: {f.width} }},' for f in recipe.fields)
    rust='''// @generated by raw_jfif_codegen.py from selected native executable/table facts.
use crate::writers::raw_segment_properties::{RawByteOrder, RawField, RawSegmentRecipe};
pub(crate) const RAW_JFIF: RawSegmentRecipe = RawSegmentRecipe {
'''+f'''    source_core_sha256: {json.dumps(fact['loaded_closure']['Image/ExifTool.pm'])},
    source_writer_sha256: {json.dumps(fact['loaded_closure']['Image/ExifTool/Writer.pl'])},
    marker: {recipe.marker}, signature: &[{signature}], skip: {recipe.skip},
    byte_order: RawByteOrder::{'Big' if recipe.byte_order=='MM' else 'Little'},
    fields: &[\n{rows}\n    ],
}};
'''
    report={'schema':1,'status':'compiled bounded raw DATAMEMBER decoder; public integration separate',
            'recipe':asdict(recipe),'native_identity':{k:v for k,v in fact['native_identity'].items() if k!='perl'},
            'loaded_closure':fact['loaded_closure'],'source_grammar_sha256':hashlib.sha256((HERE/'raw_jfif_source_grammar.json').read_bytes()).hexdigest()}
    return rust,report


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('fact',type=Path);p.add_argument('--selected-perl',required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    args=p.parse_args();rust,report=generate(json.loads(args.fact.read_text()),args.selected_perl)
    args.output.write_text(rust);args.report.write_text(json.dumps(report,sort_keys=True,indent=2)+'\n')
if __name__=='__main__': main()
