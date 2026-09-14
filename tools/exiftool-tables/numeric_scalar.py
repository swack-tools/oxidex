"""Finite numeric scalar specialization of native CheckValue/WriteValue.

Every CheckValue statement and dependency is consumed, as is the complete
numeric WriteValue graph. The runtime executes count-one int16u/rational64u
validation, integer rounding and hex conversion, unsigned fractions, and the
recognized continued-fraction Rationalize algorithm. This is not general Perl
arithmetic; unknown source statements and unrepresented input domains refuse.
"""
from dataclasses import dataclass
import json
from pathlib import Path
from checkexif_recipes import RecipeRefused, _fact
from native_reader_facts import body_tokens
from writevalue_recipes import compile_numeric_write

_CHECK = json.loads(Path(__file__).with_name('testdata').joinpath('numeric_check_source.json').read_text())

@dataclass(frozen=True)
class NumericScalarRecipe:
    formats: tuple[str, ...]
    maxima: tuple[int, ...]
    source_sha256: str
    main_source_sha256: str
    rounding_offset: float = 0.5
    rational_relative_error: float = 1e-8
    integer_range_exception: int = 0xfeedfeed


def compile_numeric_scalar(document):
    helpers = document.get('native_write_helpers', {})
    closure = document.get('native_write_capture_context', {}).get('loaded_modules', {})
    generic = {item.get('inc'): item.get('source_sha256') for item in
               document.get('native_capture_context', {}).get('loaded_closure', {}).get('modules', [])}
    packing = compile_numeric_write(helpers, closure, generic)
    check = helpers.get('check_value', {})
    provenance = _fact(check, 'numeric CheckValue', require_binding=True)
    if provenance.requested_binding != 'Image::ExifTool::CheckValue':
        raise RecipeRefused('numeric CheckValue binding is stale')
    def authenticate(actual, expected):
        name = expected['__name']
        source = expected['source_file']
        if (actual.get('__name') != name or actual.get('resolved') is not True
                or actual.get('source_file') != source
                or actual.get('source_sha256') != closure.get(source)
                or actual.get('source_sha256') != generic.get(source)):
            raise RecipeRefused('numeric CheckValue dependency source does not join: ' + name)
        if body_tokens(actual.get('__deparse')) != body_tokens(expected['__deparse']):
            raise RecipeRefused('numeric CheckValue statement grammar is unsupported: ' + name)
        dependencies = actual.get('dependencies', {})
        expected_dependencies = expected.get('dependencies', {})
        if set(dependencies) != set(expected_dependencies):
            raise RecipeRefused('numeric CheckValue dependency set changed: ' + name)
        for key in dependencies:
            authenticate(dependencies[key], expected_dependencies[key])
    authenticate(check, _CHECK)
    lexical = check.get('lexical_hashes', {})
    ranges = lexical.get('bindings', {}).get('%intRange', {})
    if lexical.get('resolved') is not True or ranges.get('resolved') is not True:
        raise RecipeRefused('numeric CheckValue intRange is unresolved')
    # The bounded native packing specialization is deliberately narrower than
    # the source integer range; changes in that range must propagate/refuse.
    if ranges.get('entries', {}).get('int16u') != ['0', '65535']:
        raise RecipeRefused('numeric CheckValue int16u range is unsupported')
    # NumericWriteRecipe authenticates a wider private WriteValue closure for
    # mandatory-directory cleanup.  The public NumericScalarRecipe is a fixed
    # two-format runtime ABI, so select its independently proven subset rather
    # than leaking that cleanup capability through positional arrays.
    public_formats = ('int16u', 'rational64u')
    if not set(public_formats).issubset(packing.formats) or not {16, 32}.issubset(packing.unsigned_bits):
        raise RecipeRefused('numeric public scalar subset is absent from authenticated WriteValue capability')
    return NumericScalarRecipe(public_formats, (65535, 4294967295), provenance.source_sha256, closure['Image/ExifTool.pm'])


def render_numeric(recipe):
    if recipe is None:
        return 'pub(crate) const NUMERIC_SCALAR: Option<NumericScalarRecipe> = None;\n'
    formats = ', '.join('"'+name+'"' for name in recipe.formats)
    maxima = ', '.join(str(value) for value in recipe.maxima)
    return f'pub(crate) const NUMERIC_SCALAR: Option<NumericScalarRecipe> = Some(NumericScalarRecipe {{ formats: [{formats}], maxima: [{maxima}], writer_source_sha256: "{recipe.source_sha256}", main_source_sha256: "{recipe.main_source_sha256}", rounding_offset: {recipe.rounding_offset}, rational_relative_error: {recipe.rational_relative_error}, integer_range_exception: {recipe.integer_range_exception} }});\n'
