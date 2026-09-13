"""Closed source contract for the shared inherited-endian unsigned reader.

This recognizes library mechanisms, not camera or tag names. Both the loaded
reader bodies and the byte-order setup body must be modeled; reading a known
function name or checking the outer caller's file is insufficient.
"""

from native_reader_facts import ReaderRefused, body_tokens, source_fact
import native_reader_facts


_GET16U = r'''($$) {
    package Image::ExifTool;
    use strict;
    (return DoUnpackStd('S', @_));
}'''

_DO_UNPACK_STD = r'''(@) {
    package Image::ExifTool;
    use strict;
    ($_[2] and (return unpack(("x$_[2] $Image::ExifTool::unpackStd{$_[0]}"), ${$_[1];})));
    (return unpack($Image::ExifTool::unpackStd{$_[0]}, ${$_[1];}));
}'''

_SET_BYTE_ORDER = r'''($) {
    package Image::ExifTool;
    use strict;
    (my $order = (shift()));
    if (($order eq 'MM')) {
        (%Image::ExifTool::unpackStd = %unpackMotorola);
    } elsif (($order eq 'II')) {
        (%Image::ExifTool::unpackStd = %unpackIntel);
    } elsif (($order =~ /^Big/i)) {
        ($order = 'MM');
        (%Image::ExifTool::unpackStd = %unpackMotorola);
    } elsif (($order =~ /^Little/i)) {
        ($order = 'II');
        (%Image::ExifTool::unpackStd = %unpackIntel);
    } else {
        (return 0);
    }
    (my $val = unpack('S', 'A '));
    my($nativeOrder);
    if (($val == 16672)) {
        ($nativeOrder = 'MM');
    } elsif (($val == 8257)) {
        ($nativeOrder = 'II');
    } else {
        warn(sprintf("Unknown native byte order! (pattern %x)\n", $val));
        (return 0);
    }
    ($Image::ExifTool::currentByteOrder = $order);
    ($Image::ExifTool::swapBytes = ($order ne $nativeOrder));
    (my $pack1d = "\000\000\000\000\000\000\360?");
    ($Image::ExifTool::swapWords = (($pack1d eq "\000\000\cO\363\000\000\000\000") || ($pack1d eq "\000\000\360?\000\000\000\000")));
    (return 1);
}'''

_GET_BYTE_ORDER = r'''() {
    package Image::ExifTool;
    use strict;
    (return $Image::ExifTool::currentByteOrder);
}'''

_FUNCTIONS = (
    ("get16u", "Image::ExifTool::Get16u", _GET16U),
    ("do_unpack_std", "Image::ExifTool::DoUnpackStd", _DO_UNPACK_STD),
    ("set_byte_order", "Image::ExifTool::SetByteOrder", _SET_BYTE_ORDER),
    ("get_byte_order", "Image::ExifTool::GetByteOrder", _GET_BYTE_ORDER),
)


def recognize(snapshot):
    """Return source facts only after every native primitive precondition holds."""
    if not isinstance(snapshot, dict) or snapshot.get("kind") != "binary_unsigned_reader_contract_v1":
        raise ReaderRefused("missing native unsigned reader contract")
    if (problem := native_reader_facts.observation_failure(snapshot)) is not None:
        raise ReaderRefused(problem)
    facts = []
    loaded = snapshot.get("loaded_functions")
    isolated = snapshot.get("isolated_functions")
    if not isinstance(loaded, dict) or not isinstance(isolated, dict):
        raise ReaderRefused("missing actual loaded reader functions")
    for key, name, expected in _FUNCTIONS:
        fact = source_fact(isolated.get(key), name)
        if fact[3] != body_tokens(expected):
            raise ReaderRefused(f"unmodeled native reader body: {name}")
        if source_fact(loaded.get(key), name) != fact:
            raise ReaderRefused("isolated reader differs from actual loaded function")
        facts.append(fact)
    by_file = {}
    for _name, file, sha, _body in facts:
        if by_file.setdefault(file, sha) != sha:
            raise ReaderRefused("native reader facts disagree about one source file")
    return tuple(facts)


def fingerprint(snapshot):
    """Fingerprint only a reader whose closed source contract holds."""
    recognize(snapshot)
    return native_reader_facts.fingerprint(snapshot)
