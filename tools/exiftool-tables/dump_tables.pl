#!/usr/bin/env perl
#
# Dump ExifTool's tag tables to JSON by loading the modules and walking the
# symbol table.
#
# This is deliberately NOT a Perl parser.  ExifTool's tables are built at
# require-time -- some are assembled by loops, some inherit via %$tagTablePtr
# copies, some are patched by an END block.  Any regex over the .pm text sees
# the source, not the table that ExifTool actually dispatches on.  Loading the
# module and reading the resulting hash is the only way to get the real thing,
# and it costs nothing: the tables are already in memory once `require` returns.
#
# Every value lands in one of two buckets:
#
#   data  -- integers, strings, enum maps.  Reproducible in Rust exactly.
#   perl  -- a code ref or an expression string ('$val * 2', 'Image::...').
#            Recorded verbatim as {"__perl": "..."} and NEVER guessed at.
#
# The codegen refuses to emit any tag whose conversions land in the `perl`
# bucket unless a translation is registered for that exact expression.  A
# plausible-looking wrong number under a real ExifTool tag name is worse than
# an absent tag, so unsupported means omitted-and-counted, not approximated.

use strict;
use warnings;
use JSON::PP;
use Encode qw(decode);
use Digest::SHA qw(sha256_hex);
use Cwd qw(abs_path);
use File::Spec ();
use File::Basename qw(dirname);
use FindBin;
use lib $FindBin::Bin;
use OxiDex::NativeReaderContract ();
use Scalar::Util qw(refaddr);
use B ();

# Resolve a code ref to its fully-qualified sub name.
#
# Worth the trouble because PROCESS_PROC is how a table declares what kind of
# thing it is.  Recording it as an opaque "CODE" throws that away, and the
# generator then cannot tell a ProcessBinaryData table (a flat record with a
# field per offset -- mechanically transcribable) from an IFD or a bespoke
# parser.  ExifTool's own dispatch keys off exactly this.
sub code_name {
    my ($cv) = @_;
    my $b = eval { B::svref_2object($cv) } or return undef;
    return undef unless $b->isa('B::CV');
    my $gv = eval { $b->GV } or return undef;
    return undef if ref($gv) eq 'B::SPECIAL';
    my $stash = eval { $gv->STASH->NAME } // '';
    my $name  = eval { $gv->NAME } // '';
    return undef unless $name;
    return $stash ? "${stash}::${name}" : $name;
}

# Recover the body of an anonymous sub as source text.
#
# A named sub is identified by its name; an anonymous one is not identified by
# anything at all, and a PrintConv's `OTHER => sub {...}` is always anonymous.
# Recording it as an opaque "CODE" leaves a downstream generator with no way to
# tell `sub { return $_[0] }` from `sub { "$val fps" }` -- and something has to
# tell them apart, because they are not the same tag value.  Deparsing turns the
# closure back into text that a translation registry can be *keyed on*, so a
# translation is bound to the exact code it was written against and an upstream
# edit shows up as an unknown key rather than as a silently stale conversion.
sub deparse {
    my ($cv) = @_;
    return undef unless eval { require B::Deparse; 1 };
    my $text = eval { B::Deparse->new('-p', '-sC')->coderef2text($cv) };
    return defined $text ? to_text($text) : undef;
}

# `SubDirectory.Validate` is normally a Perl expression string.  A fully
# qualified call in that string names a helper whose body controls whether the
# child is entered, so its name alone is not useful to a downstream compiler.
# Record the loaded CODE ref and the exact selected-library source file here.
# This is source evidence only: consumers must still recognise the body and
# call arguments conservatively, and must refuse `resolved => false` facts.
sub fully_qualified_calls {
    my ($expr) = @_;
    return () unless defined $expr && !ref $expr;
    my %seen;
    # This deliberately identifies calls, not arbitrary package tokens.  It
    # does not parse or evaluate the enclosing Perl expression.
    while ($expr =~ /(?<![\w:])((?:[A-Za-z_]\w*::)+[A-Za-z_]\w*)\s*\(/g) {
        $seen{$1} = 1;
    }
    return sort keys %seen;
}

sub source_file_fact {
    my ($cv, $lib_abs) = @_;
    my $file = eval { B::svref_2object($cv)->FILE };
    return (undef, undef, 'source_file_unavailable') unless defined $file && length $file;
    my $abs = abs_path($file);
    return (undef, undef, 'source_file_unreadable') unless defined $abs && -f $abs;
    my $prefix = $lib_abs . '/';
    return (undef, undef, 'source_outside_selected_lib')
        unless index($abs, $prefix) == 0;
    open(my $fh, '<:raw', $abs) or return (undef, undef, 'source_file_unreadable');
    local $/;
    my $bytes = <$fh>;
    close($fh) or return (undef, undef, 'source_file_unreadable');
    return (File::Spec->abs2rel($abs, $lib_abs), sha256_hex($bytes), undef);
}

sub unresolved_code_fact {
    my ($name, $reason) = @_;
    return {
        __perl => 'CODE', __opaque => JSON::PP::true, __name => $name,
        resolved => JSON::PP::false, __deparse => undef,
        source_file => undef, source_sha256 => undef, reason => $reason,
    };
}

sub direct_code_dependencies {
    my ($body, $owner) = @_;
    my %calls = map { $_ => 1 } fully_qualified_calls($body);
    my $package = $owner;
    $package =~ s/::[A-Za-z_]\w*$//;
    # B::Deparse leaves same-package calls unqualified. Retain only names
    # that resolve to an actual CODE glob in that package; Perl builtins and
    # control keywords therefore do not become invented dependencies.
    while ($body =~ /(?<![\w:])([A-Za-z_]\w*)\s*\(/g) {
        my $bare = $1;
        next if $bare =~ /^(?:if|unless|while|until|for|foreach|return|my|our|state|sub|package|use)$/;
        my $candidate = "${package}::${bare}";
        no strict 'refs';
        $calls{$candidate} = 1 if *{$candidate}{CODE};
    }
    return sort keys %calls;
}

# Capture the actual CV stored by a table, then resolve its direct calls only
# after all requested modules loaded.  PROCESS_PROC is a CODE reference in the
# table, so looking its name up again would silently authenticate a later
# redefinition rather than the callable table value.  Bare calls, by contrast,
# use the final package glob at execution time; recording that binding exposes
# a local `Get16u` replacement without assigning vendor meaning to it.
sub code_ref_fact {
    my ($cv, $fallback_name, $lib_abs, $ancestors, $depth, $capture_dependencies) = @_;
    $capture_dependencies = 1 unless defined $capture_dependencies;
    $ancestors //= {};
    $depth //= 0;
    my $display_name = defined $fallback_name ? $fallback_name : '';
    return unresolved_code_fact($display_name, 'dependency_depth_exceeded') if $depth > 8;
    my %fact = (
        __perl  => 'CODE',
        __opaque => JSON::PP::true,
        __name  => $display_name,
        resolved => JSON::PP::false,
        __deparse => undef,
        source_file => undef,
        source_sha256 => undef,
    );
    return { %fact, reason => 'code_ref_unavailable' } unless $cv;
    my $resolved_name = code_name($cv);
    return { %fact, reason => 'code_name_unavailable' } unless defined $resolved_name;
    return unresolved_code_fact($resolved_name, 'dependency_cycle') if $ancestors->{$resolved_name};
    $fact{__name} = $resolved_name;
    my $body = deparse($cv);
    return { %fact, reason => 'deparse_unavailable' } unless defined $body;
    $fact{__deparse} = $body;
    my ($source_file, $source_sha256, $source_error) = source_file_fact($cv, $lib_abs);
    return { %fact, reason => $source_error } if defined $source_error;
    $fact{source_file} = $source_file;
    $fact{source_sha256} = $source_sha256;
    $fact{resolved} = JSON::PP::true;
    my %next_ancestors = (%$ancestors, $resolved_name => 1);
    my %dependencies;
    if ($capture_dependencies) {
        for my $callee (direct_code_dependencies($body, $resolved_name)) {
            $dependencies{$callee} = code_source_fact(
                $callee, $lib_abs, \%next_ancestors, $depth + 1);
        }
    } else {
        # A PROCESS_PROC needs only the actual package-local reader it invokes,
        # not the entire closure of common ProcessBinaryData helpers repeated
        # once per table.  The reader contract owns the transitive primitive
        # evidence.  A missing binding remains explicit so consumers refuse.
        my $package = $resolved_name;
        $package =~ s/::[A-Za-z_]\w*$//;
        if ($body =~ /(?<![\w:>])Get16u\s*\(/) {
            my $binding = "${package}::Get16u";
            $dependencies{$binding} = code_source_fact(
                $binding, $lib_abs, \%next_ancestors, $depth + 1, 0);
        }
    }
    $fact{dependencies} = \%dependencies if %dependencies;
    return \%fact;
}

sub code_source_fact {
    my ($name, $lib_abs, $ancestors, $depth, $capture_dependencies) = @_;
    $capture_dependencies = 1 unless defined $capture_dependencies;
    $ancestors //= {};
    $depth //= 0;
    return unresolved_code_fact($name, 'dependency_depth_exceeded') if $depth > 8;
    return unresolved_code_fact($name, 'dependency_cycle') if $ancestors->{$name};
    return unresolved_code_fact($name, 'invalid_fully_qualified_name')
        unless $name =~ /^(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*$/;
    no strict 'refs';
    my $cv = *{$name}{CODE};
    return unresolved_code_fact($name, 'code_ref_unavailable') unless $cv;
    return code_ref_fact($cv, $name, $lib_abs, $ancestors, $depth, $capture_dependencies);
}

sub validate_function_fact {
    my ($name, $lib_abs) = @_;
    return code_source_fact($name, $lib_abs);
}

sub collect_subdirectory_validate_function_names {
    my ($value, $names, $seen, $depth) = @_;
    return if !defined $value || $depth > 24;
    my $kind = ref $value;
    return unless $kind eq 'HASH' || $kind eq 'ARRAY';
    my $id = refaddr($value);
    return if defined $id && $seen->{$id}++;
    if ($kind eq 'HASH') {
        my $subdir = $value->{SubDirectory};
        if (ref($subdir) eq 'HASH') {
            for my $name (fully_qualified_calls($subdir->{Validate})) {
                $names->{$name} = 1;
            }
        }
        collect_subdirectory_validate_function_names($_, $names, $seen, $depth + 1)
            for values %$value;
    } else {
        collect_subdirectory_validate_function_names($_, $names, $seen, $depth + 1)
            for @$value;
    }
}

# ExifTool's sources are a mix of ASCII, UTF-8 and Latin-1 (copyright signs in
# Notes, accented names in manufacturer tables).  Perl hands us bytes; JSON must
# be valid UTF-8.  Decode as UTF-8 where that succeeds and fall back to
# Latin-1, which cannot fail, rather than dropping or replacing the character.
sub to_text {
    my ($s) = @_;
    return $s unless defined $s && !ref $s;
    return $s if utf8::is_utf8($s);
    my $d = eval { decode('UTF-8', $s, Encode::FB_CROAK) };
    return defined $d ? $d : decode('ISO-8859-1', $s);
}

my $EXIFTOOL_LIB = shift @ARGV or die "usage: $0 <exiftool-lib-dir> [module...]\n";
unshift @INC, $EXIFTOOL_LIB;

my $EXIFTOOL_LIB_ABS = abs_path($EXIFTOOL_LIB) or die "invalid exiftool lib: $EXIFTOOL_LIB\n";

require Image::ExifTool;

# Keys that describe the table itself rather than a tag within it.
my %TABLE_META = map { $_ => 1 } qw(
    GROUPS WRITE_PROC CHECK_PROC PROCESS_PROC WRITABLE NOTES FORMAT
    FIRST_ENTRY DATAMEMBER VARS PRIORITY TAG_PREFIX WRITE_GROUP
    SET_GROUP1 PREFERRED IS_OFFSET IS_SUBDIR NAMESPACE PARSE_PROC
    AVOID LANG_INFO DID_TAG_ID PERMANENT INIT_TABLE
);

# Per-tag keys worth carrying across.  Anything not listed is dropped rather
# than half-understood; add deliberately, after checking what it means.
#
# BitShift travels with Mask and is not optional.  ExifTool reduces a masked
# field to `($val & Mask) >> BitShift` (ExifTool.pm ProcessBinaryData), and
# derives BitShift from the lowest set bit of Mask only when the table does not
# state one (ExifTool.pm, "calculate BitShift from Mask if necessary").  A
# generator that always derives it is right almost everywhere and quietly wrong
# where a table overrides it -- BPG::Main 4.1 `Alpha` declares Mask 0x1004 with
# BitShift 0, where deriving would give 2 and shift every enum key off its
# meaning.  Carrying the key is what keeps the transcription exact instead of
# merely plausible.
#
# FixFormat and SubIFD (slice I-1, the IFD-style tables) are tag-level keys,
# siblings of `Flags => 'SubIFD'` on the sub-IFD alternatives of Olympus.pm's
# 0x2010-0x5000 entries and on Exif.pm's ExifOffset (0x8769).  SubIFD is the
# un-sugared form of that Flags entry -- ProcessExif reads `$$tagInfo{SubIFD}`
# (Exif.pm:6724, :6733, :6747, :7058) after SetupTagTable has expanded Flags
# into it -- so a tag that declares it directly must be carried, not left in
# `_extra_keys` where only its presence survives.  FixFormat is read by
# WriteExif.pl:1760 alone (README:930: "[Writable EXIF SubIFD SubDirectory's
# only]"); the IFD schema carries it as data so the transcription is complete,
# and codegen.py says so where it emits it.  Keys inside a SubDirectory hash
# (MaxSubdirs, DirName, ...) need no entry here: `scrub` copies that hash whole.
my @TAG_KEYS = qw(
    Name Description Format Writable Count Groups Notes Mask BitShift Condition
    PrintConv ValueConv RawConv PrintConvInv ValueConvInv Hook
    SubDirectory Flags Unknown Hidden Avoid Binary Protected List
    Priority ByteOrder DataMember RelatedTag SeparateTable PrintHex
    Base Offset ChangeBase
    Require Desire Inhibit
    BitsPerWord BitsTotal
    FixFormat SubIFD
);

# Write routing is deliberately captured beside, rather than inside, the read
# projection above.  Read codegen has a closed grammar and must not become more
# permissive merely because a source row names a write-only control.  The
# sidecar below retains the raw source values for a later write compiler to
# authenticate and model independently.
my @WRITE_CONTROL_KEYS = qw(
    Writable WriteGroup RawConvInv ValueConvInv PrintConvInv
    Validate Mandatory DelValue Deletable WriteAlso WriteCheck WriteCondition
    WriteHook WriteLast WritePseudo CanCreate
);
my %WRITE_KNOWN_TAG_KEY = map { $_ => 1 } (@TAG_KEYS, @WRITE_CONTROL_KEYS);

sub scrub {
    my ($v, $depth) = @_;
    $depth //= 0;
    return undef unless defined $v;
    return { __deep => 1 } if $depth > 12;

    my $r = ref $v;
    if (!$r) {
        # A bare string in a conversion slot is a Perl expression, but we
        # cannot tell that here -- the caller tags it by field name.
        return to_text($v);
    }
    if ($r eq 'CODE')   {
        my $n = code_name($v);
        my $src = deparse($v);
        return {
            __perl => 'CODE', __opaque => 1,
            defined $n   ? (__name => $n)      : (),
            defined $src ? (__deparse => $src) : (),
        };
    }
    if ($r eq 'SCALAR') { return scrub($$v, $depth + 1) }
    if ($r eq 'ARRAY')  { return [ map { scrub($_, $depth + 1) } @$v ] }
    if ($r eq 'HASH') {
        my %out;
        for my $k (keys %$v) {
            $out{to_text($k)} = scrub($v->{$k}, $depth + 1);
        }
        return \%out;
    }
    return { __ref => $r };
}

# A PrintConv/ValueConv is either an enum map (pure data, directly usable) or
# an expression (must be translated by hand).  Distinguishing the two is the
# single most valuable thing this script does: enum maps are ~60% of the
# entries and are 100% mechanically safe.
sub classify_conv {
    my ($v) = @_;
    return undef unless defined $v;
    my $r = ref $v;

    if ($r eq 'HASH') {
        # Keys like BITMASK/OTHER/Notes are directives, not enum values.
        my %map;
        my %directive;
        for my $k (keys %$v) {
            if ($k =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/) {
                $directive{$k} = scrub($v->{$k});
                next;
            }
            my $val = $v->{$k};
            if (ref $val) { $directive{$k} = scrub($val); next }
            $map{to_text($k)} = to_text($val);
        }
        return {
            kind      => (%directive ? 'enum_partial' : 'enum'),
            map       => \%map,
            directives=> (%directive ? \%directive : undef),
        };
    }
    if ($r eq 'CODE')  { return { kind => 'code', expr => undef, deparse => deparse($v) } }
    if ($r eq 'ARRAY') { return { kind => 'list', items => scrub($v) } }
    if (!$r)           { return { kind => 'expr', expr => to_text($v) } }
    return { kind => 'other', dump => scrub($v) };
}

# The live table graph contains runtime references such as Composite tags'
# `Table` back-pointer.  They are not serializable source operands and may be
# cyclic.  Preserve their reference kind explicitly while keeping ordinary
# source hashes, arrays, scalar refs, CODE, undef, zero, and empty strings
# distinct.  This is intentionally separate from read-side `scrub`, whose
# historical output must remain byte-for-byte stable.
sub write_scrub {
    my ($v, $depth, $seen) = @_;
    $depth //= 0;
    $seen //= {};
    return undef unless defined $v;
    return { __deep => 1 } if $depth > 12;
    my $kind = ref($v);
    return to_text($v) unless $kind;
    if ($kind eq 'CODE') {
        my $name = code_name($v);
        my $source = deparse($v);
        return {
            __perl => 'CODE', __opaque => JSON::PP::true,
            (defined $name ? (__name => $name) : ()),
            (defined $source ? (__deparse => $source) : ()),
        };
    }
    my $id = refaddr($v);
    if ($kind eq 'SCALAR') {
        return { __ref => 'SCALAR', __cycle => JSON::PP::true }
            if defined $id && $seen->{$id};
        $seen->{$id} = 1 if defined $id;
        my $out = { __ref => 'SCALAR', value => write_scrub($$v, $depth + 1, $seen) };
        delete $seen->{$id} if defined $id;
        return $out;
    }
    return { __ref => $kind, __cycle => JSON::PP::true }
        if defined $id && $seen->{$id};
    $seen->{$id} = 1 if defined $id;
    my $out;
    if ($kind eq 'ARRAY') {
        $out = [ map { write_scrub($_, $depth + 1, $seen) } @$v ];
    } elsif ($kind eq 'HASH') {
        my %hash;
        for my $key (sort keys %$v) {
            $hash{to_text($key)} = write_scrub($v->{$key}, $depth + 1, $seen);
        }
        $out = \%hash;
    } else {
        $out = { __ref => $kind };
    }
    delete $seen->{$id} if defined $id;
    return $out;
}

# Write facts preserve the original value even for a conversion.  The
# classification is a convenience for a future compiler, never a replacement
# for the raw source operand.  In particular, a present-but-undef value is
# distinguishable from an absent key through the surrounding `present` flag.
sub write_source_property {
    my ($hash, $key) = @_;
    return { present => JSON::PP::false } unless exists $hash->{$key};
    my %property = (
        present => JSON::PP::true,
        value => write_scrub($hash->{$key}),
    );
    if ($key =~ /^(?:RawConvInv|ValueConvInv|PrintConvInv)$/) {
        $property{classification} = classify_conv($hash->{$key});
    }
    return \%property;
}

sub is_table_property {
    my ($key) = @_;
    no warnings 'once';
    return $TABLE_META{$key} || $Image::ExifTool::specialTags{$key};
}

sub dump_write_entry {
    my ($entry) = @_;
    my $kind = ref($entry) || 'SCALAR';
    if (!$entry || !ref($entry)) {
        return {
            entry_kind => $kind,
            value => scrub($entry),
        };
    }
    if ($kind eq 'ARRAY') {
        return {
            entry_kind => 'ARRAY',
            # Array order is native variant-selection order.  Do not sort it.
            alternatives => [ map { dump_write_entry($_) } @$entry ],
        };
    }
    return { entry_kind => $kind, value => scrub($entry) } unless $kind eq 'HASH';

    my %properties;
    my %unknown;
    for my $key (sort keys %$entry) {
        my $property = write_source_property($entry, $key);
        $properties{to_text($key)} = $property;
        $unknown{to_text($key)} = $property unless $WRITE_KNOWN_TAG_KEY{$key};
    }
    my %controls = map { $_ => write_source_property($entry, $_) } @WRITE_CONTROL_KEYS;
    return {
        entry_kind => 'HASH',
        properties => \%properties,
        write_controls => \%controls,
        unknown_properties => \%unknown,
    };
}

sub dump_write_table {
    my ($module, $table, $full_name, $hash) = @_;
    my (%properties, %unknown);
    for my $key (sort keys %$hash) {
        next unless is_table_property($key);
        my $property = write_source_property($hash, $key);
        $properties{to_text($key)} = $property;
        $unknown{to_text($key)} = $property unless $TABLE_META{$key};
    }
    my %controls = map { $_ => write_source_property($hash, $_) }
        qw(WRITABLE WRITE_GROUP WRITE_PROC CHECK_PROC);
    my %rows;
    for my $key (sort keys %$hash) {
        next if is_table_property($key) || $key =~ /^_/;
        $rows{to_text($key)} = dump_write_entry($hash->{$key});
    }
    return {
        module => $module,
        table => $table,
        full_name => $full_name,
        table_properties => \%properties,
        write_controls => \%controls,
        unknown_table_properties => \%unknown,
        rows => \%rows,
        row_count => scalar(keys %rows),
    };
}

# ExifTool's write functions are often only prototype declarations while an
# input table is being loaded.  Its AUTOLOAD convention derives the file from
# the fully-qualified function name.  Requiring that file is a load operation,
# not a call: it cannot run an arbitrary writer against dummy metadata.  We do
# this only for a prototype-only CV; normal loaded procedures keep their actual
# table-owned binding unchanged.
sub prototype_only_code {
    my ($cv) = @_;
    my $body = deparse($cv);
    return 0 unless defined $body;
    return $body =~ /^\s*\([^{};]*\)\s*;\s*$/s ? 1 : 0;
}

sub write_autoload_file {
    my ($name) = @_;
    return undef unless defined $name && $name =~ /^(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*$/;
    my @part = split /::/, $name;
    return undef unless @part >= 3 && $part[0] eq 'Image' && $part[1] eq 'ExifTool';
    return "Image/ExifTool/Write$part[2].pl" if @part == 4;
    return 'Image/ExifTool/Shift.pl' if $part[-1] eq 'ShiftTime';
    return 'Image/ExifTool/Writer.pl';
}

# ExifTool routes prototype writer declarations through DoAutoLoad.  The
# dumper may load an implementation only if it sees the closed, generic routing
# body below in the *actual loaded* core CV.  This protects the sidecar from
# claiming an implementation after an upstream routing change; it does not
# treat the routing fact as writer admission.
sub write_autoload_router_fact {
    my ($lib_abs) = @_;
    no strict 'refs';
    my $cv = *{'Image::ExifTool::DoAutoLoad'}{CODE};
    return unresolved_code_fact('Image::ExifTool::DoAutoLoad', 'code_ref_unavailable') unless $cv;
    return code_ref_fact($cv, 'Image::ExifTool::DoAutoLoad', $lib_abs, undef, undef, 0);
}

sub write_autoload_router_tokens {
    my ($body) = @_;
    return undef unless defined $body;
    my @tokens;
    pos($body) = 0;
    while (pos($body) < length($body)) {
        if ($body =~ /\G\s+/gc) {
            next;
        } elsif ($body =~ /\G((?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'))/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G(\/(?:\\.|[^\/\\])*\/)/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G(\$\#?[A-Za-z_]\w*|\$\@|\@(?:[A-Za-z_]\w*|_))/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G([A-Za-z_]\w*(?:::[A-Za-z_]\w*)*)/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G(\d+)/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G(\.=|==)/gc) {
            push @tokens, $1;
        } elsif ($body =~ /\G([(){}\[\];,.=&@-])/gc) {
            push @tokens, $1;
        } else {
            return undef;
        }
    }
    return \@tokens;
}

sub write_autoload_router_expected_tokens {
    # Full B::Deparse token grammar for the generic native DoAutoLoad route:
    # DESTROY guard; 4-part, ShiftTime, and Writer file paths; guarded require;
    # implementation check; and tail call. Whitespace is immaterial, but every
    # executable token is consumed, so an inserted assignment or early return
    # is not mistaken for the native dispatcher.
    my $canonical = <<'END_AUTOLOAD';
(@) {
package Image::ExifTool;
use strict;
(my($autoload) = (shift()));
(my @callInfo = split(/::/, $autoload, 0));
(my($file) = 'Image/ExifTool/Write');
(($callInfo[$#callInfo] eq 'DESTROY') and (return));
if ((@callInfo == 4)) {
($file .= "$callInfo[2].pl");
} elsif (($callInfo[-1] eq 'ShiftTime')) {
($file = 'Image/ExifTool/Shift.pl');
} else {
($file .= 'r.pl');
}
(eval {
do {
(require $file)
}
} or die(("Error while attempting to call $autoload\n$@\n")));
unless (defined(&$autoload)) {
(my(@caller) = caller(0));
die(("Undefined subroutine $autoload called at $caller[1] line $caller[2]\n"));
}
no strict 'refs';
(return &$autoload(@_));
}
END_AUTOLOAD
    return write_autoload_router_tokens($canonical);
}

sub write_autoload_router_supported {
    my ($fact) = @_;
    return 0 unless $fact->{resolved} && ($fact->{__name} // '') eq 'Image::ExifTool::DoAutoLoad';
    my $actual = write_autoload_router_tokens($fact->{__deparse});
    my $expected = write_autoload_router_expected_tokens();
    return 0 unless $actual && $expected && @$actual == @$expected;
    for my $i (0 .. $#$expected) {
        return 0 unless $actual->[$i] eq $expected->[$i];
    }
    return 1;
}

sub hydrate_write_procedures {
    my ($tables, $router) = @_;
    my %status;
    for my $full_name (sort keys %$tables) {
        my $hash = $tables->{$full_name}{hash};
        for my $key (qw(WRITE_PROC CHECK_PROC)) {
            next unless ref($hash->{$key}) eq 'CODE';
            my $cv = $hash->{$key};
            next unless prototype_only_code($cv);
            my $name = code_name($cv);
            my $file = write_autoload_file($name);
            my $id = refaddr($cv);
            if (!$router->{supported}) {
                $status{$id} = { reason => 'autoload_router_unsupported' };
                next;
            }
            if (!defined $file) {
                $status{$id} = { reason => 'autoload_target_unavailable' };
                next;
            }
            my $loaded = eval { require $file; 1 };
            if (!$loaded) {
                $status{$id} = { reason => 'autoload_load_failed', autoload_file => $file };
                next;
            }
            # A successful require which leaves the stored CV as a prototype
            # is still not an implementation fact.
            if (prototype_only_code($cv)) {
                $status{$id} = { reason => 'autoload_did_not_define', autoload_file => $file };
            } else {
                $status{$id} = { autoload_file => $file };
            }
        }
    }
    return \%status;
}

sub effective_write_code_fact {
    my ($hash, $key, $fallback_name, $lib_abs, $status) = @_;
    return { present => JSON::PP::false } unless exists $hash->{$key};
    my $value = $hash->{$key};
    my $declared = write_source_property($hash, $key);
    if (ref($value) ne 'CODE') {
        return {
            present => JSON::PP::true,
            declared => $declared,
            effective => unresolved_code_fact($fallback_name, 'write_proc_not_code'),
        };
    }
    my $load = $status->{refaddr($value)};
    if ($load && $load->{reason}) {
        my $fact = unresolved_code_fact(code_name($value) // $fallback_name, $load->{reason});
        $fact->{autoload_file} = $load->{autoload_file} if exists $load->{autoload_file};
        return { present => JSON::PP::true, declared => $declared, effective => $fact };
    }
    if (prototype_only_code($value)) {
        return {
            present => JSON::PP::true,
            declared => $declared,
            effective => unresolved_code_fact(code_name($value) // $fallback_name,
                'prototype_only_write_proc'),
        };
    }
    # Capture only the bounded package-local reader dependency, as for
    # PROCESS_PROC.  Writer admission still needs a distinct body recognizer.
    return {
        present => JSON::PP::true,
        declared => $declared,
        effective => code_ref_fact($value, $fallback_name, $lib_abs, undef, undef, 0),
        ($load && exists $load->{autoload_file} ? (autoload_file => $load->{autoload_file}) : ()),
    };
}

# These helpers supply the first UTF-8 scalar writer's value validation and
# serialization path.  Capture their *final loaded* bindings separately from
# table WRITE_PROC/CHECK_PROC provenance: a later mechanism compiler must
# recognize their bodies before it can execute them.  code_source_fact keeps
# the established depth/cycle limits and makes a missing helper explicit.
sub hydrate_write_helpers {
    # Writer.pl defines the shared WriteValue/CheckValue helpers but is not
    # necessarily loaded by a table's WriteExif implementation.  This is a
    # module load only, after all requested table modules are captured; it
    # never invokes a writer against metadata.
    return { loaded => JSON::PP::true } if $INC{'Image/ExifTool/Writer.pl'};
    return { loaded => JSON::PP::true } if eval { require 'Image/ExifTool/Writer.pl'; 1 };
    return { loaded => JSON::PP::false, reason => 'write_helper_load_failed' };
}

sub native_write_helper_facts {
    my ($lib_abs, $status) = @_;
    if (!$status->{loaded}) {
        my $reason = $status->{reason} // 'write_helper_load_failed';
        return {
            write_value => unresolved_code_fact('Image::ExifTool::WriteValue', $reason),
            check_value => unresolved_code_fact('Image::ExifTool::CheckValue', $reason),
        };
    }
    return {
        write_value => code_source_fact('Image::ExifTool::WriteValue', $lib_abs),
        check_value => code_source_fact('Image::ExifTool::CheckValue', $lib_abs),
    };
}

sub dump_tag_entry {
    my ($entry) = @_;
    my $r = ref $entry;

    # Bare string: shorthand for { Name => '...' }
    return { Name => to_text($entry), _shorthand => JSON::PP::true } if !$r;

    # Arrayref: conditional variants, tried in order.  This is how ExifTool
    # models model-dependent layouts (Canon CameraInfo's 33 alternatives).
    if ($r eq 'ARRAY') {
        return {
            _variants => [ map { dump_tag_entry($_) } @$entry ],
        };
    }
    return { _unhandled => $r } unless $r eq 'HASH';

    my %out;
    for my $k (@TAG_KEYS) {
        next unless exists $entry->{$k};
        my $v = $entry->{$k};
        if ($k =~ /^(PrintConv|ValueConv|RawConv|PrintConvInv|ValueConvInv)$/) {
            $out{$k} = classify_conv($v);
        } elsif ($k eq 'SubDirectory') {
            my $sd = scrub($v);
            # TagTable is the edge in the table graph -- what makes whole-table
            # extraction possible instead of tag-at-a-time guessing.
            $out{SubDirectory} = $sd;
        } else {
            $out{$k} = scrub($v);
        }
    }
    # Record unknown keys so the schema can grow deliberately instead of
    # silently losing information.
    my @unknown = grep { !exists $out{$_} && !$TABLE_META{$_} } keys %$entry;
    @unknown = grep { my $k = $_; !grep { $_ eq $k } @TAG_KEYS } @unknown;
    $out{_extra_keys} = [ sort @unknown ] if @unknown;
    return \%out;
}

sub dump_module {
    my ($module, $validate_function_names, $processor_tables, $write_tables) = @_;
    my $pkg = "Image::ExifTool::$module";
    eval "require $pkg; 1" or do {
        return { module => $module, error => "$@" };
    };

    no strict 'refs';
    my $stash = \%{"${pkg}::"};
    my %tables;

    for my $sym (sort keys %$stash) {
        next if $sym =~ /::$/;             # nested stash
        my $glob = $stash->{$sym};
        next unless ref(\$glob) eq 'GLOB' || ref($glob) eq 'GLOB';
        my $hash = eval { \%{"${pkg}::${sym}"} };
        next unless $hash && ref $hash eq 'HASH' && keys %$hash;

        # A tag table has either table-level metadata or tag-ish keys.
        my @keys = keys %$hash;
        my @tagkeys = grep { !$TABLE_META{$_} && !/^_/ } @keys;
        my $has_meta = grep { $TABLE_META{$_} } @keys;
        next unless $has_meta || @tagkeys;

        # Reject obvious non-tables (lookup hashes of plain scalars with no
        # metadata) -- they are conversion data, useful but not tag tables.
        my $struct_vals = grep { ref $hash->{$_} } @tagkeys;
        next unless $has_meta || $struct_vals;

        my %tags;
        for my $k (@tagkeys) {
            collect_subdirectory_validate_function_names(
                $hash->{$k}, $validate_function_names, {}, 0);
            $tags{$k} = dump_tag_entry($hash->{$k});
        }
        my %meta;
        for my $k (grep { $TABLE_META{$_} } @keys) {
            $meta{$k} = scrub($hash->{$k});
        }
        # Preserve the table's actual PROCESS_PROC CV.  Its complete source
        # fact is filled after every module has loaded, when package-local
        # bare-reader bindings are final.
        if (ref $hash->{PROCESS_PROC} eq 'CODE') {
            $processor_tables->{"${pkg}::${sym}"} = {
                cv => $hash->{PROCESS_PROC}, module => $module, table => $sym,
            };
        }
        # The write sidecar owns a raw reference to every live tag table, not
        # only tables which currently look writable.  A later compiler must be
        # able to tell absent controls from an unsupported or newly introduced
        # one, and zero-row tables are source facts too.
        $write_tables->{"${pkg}::${sym}"} = {
            hash => $hash, module => $module, table => $sym,
        };

        $tables{$sym} = {
            full_name => "${pkg}::${sym}",
            meta      => \%meta,
            tags      => \%tags,
            tag_count => scalar(keys %tags),
        };
    }

    # ARRAY package variables -- e.g. @Image::ExifTool::MakerNotes::Main, an
    # ordered list of routing entries tried in order until one's Condition
    # matches. A hash table has no concept of "try these in this order until
    # one wins"; MakerNotes::Main IS that concept, so it has to be an array,
    # and until now dump_tables.pl only ever walked the stash's HASH globs --
    # this array (94 rows) was invisible to it entirely.
    #
    # Only arrays whose elements are refs (HASH, or ARRAY-of-alternatives, the
    # same %$tagTablePtr shapes dump_tag_entry already understands for a
    # single hash entry) are kept. An array of bare scalars is enum/constant
    # data, not a routing or tag table, mirroring the $struct_vals gate above
    # for hashes.
    my %arrays;
    for my $sym (sort keys %$stash) {
        next if $sym =~ /::$/;
        my $glob = $stash->{$sym};
        next unless ref(\$glob) eq 'GLOB' || ref($glob) eq 'GLOB';
        my $aref = eval { \@{"${pkg}::${sym}"} };
        next unless $aref && ref $aref eq 'ARRAY' && @$aref;
        next unless grep { ref $_ } @$aref;

        collect_subdirectory_validate_function_names(
            $aref, $validate_function_names, {}, 0);
        my @rows = map { dump_tag_entry($_) } @$aref;
        $arrays{$sym} = {
            full_name => "${pkg}::${sym}",
            rows      => \@rows,
            row_count => scalar(@rows),
        };
    }

    return {
        module      => $module,
        package     => $pkg,
        tables      => \%tables,
        table_count => scalar(keys %tables),
        arrays      => \%arrays,
        array_count => scalar(keys %arrays),
    };
}

# ---------------------------------------------------------------------------

my @modules = @ARGV;
unless (@modules) {
    opendir(my $dh, "$EXIFTOOL_LIB/Image/ExifTool") or die "opendir: $!";
    @modules = sort map { s/\.pm$//r } grep { /\.pm$/ } readdir($dh);
    closedir $dh;
    # These are machinery, not tag tables.
    my %skip = map { $_ => 1 } qw(
        BuildTagLookup TagLookup TagNames Writer Shift Import
        Validate Geolocation
    );
    @modules = grep { !$skip{$_} } @modules;
}

my %out;
my %subdirectory_validate_function_names;
my %subdirectory_validate_functions;
my %processor_tables;
my %write_tables;
my %native_write_tables;
my ($ok, $failed) = (0, 0);
for my $m (@modules) {
    my $r = dump_module($m, \%subdirectory_validate_function_names, \%processor_tables, \%write_tables);
    if ($r->{error}) {
        $failed++;
        warn "SKIP $m: $r->{error}";
        next;
    }
    next unless $r->{table_count} || $r->{array_count};
    $out{$m} = $r;
    $ok++;
}

# Resolve after every requested module has loaded: a table may name a helper
# defined by a later module, and later source may replace an earlier CODE ref.
for my $name (sort keys %subdirectory_validate_function_names) {
    $subdirectory_validate_functions{$name} = validate_function_fact(
        $name, $EXIFTOOL_LIB_ABS);
}

# Replace the shallow PROCESS_PROC scrub with the actual table CV provenance
# and final package-local dependencies.  Keeping the fact at the table's own
# metadata path lets a consumer join source identity to the processor without
# a vendor/table switch.  Every table retains its own fact even when several
# tables share a CODE ref.
for my $full_name (sort keys %processor_tables) {
    my $entry = $processor_tables{$full_name};
    my $fact = code_ref_fact($entry->{cv}, "${full_name}::PROCESS_PROC", $EXIFTOOL_LIB_ABS, undef, undef, 0);
    $out{$entry->{module}}{tables}{$entry->{table}}{meta}{PROCESS_PROC} = $fact;
}

# Resolve prototype-only writer declarations after every selected module is
# loaded.  This is intentionally after the read dump was built: writer module
# loading must not alter the existing read projection.  The sidecar gets the
# table's stored CV, its final implementation source fact, and raw source
# controls separately.
my $write_autoload_router = write_autoload_router_fact($EXIFTOOL_LIB_ABS);
my $write_autoload_router_status = {
    router => $write_autoload_router,
    supported => write_autoload_router_supported($write_autoload_router) ? JSON::PP::true : JSON::PP::false,
};
my $write_autoload_status = hydrate_write_procedures(\%write_tables, $write_autoload_router_status);
for my $full_name (sort keys %write_tables) {
    my $entry = $write_tables{$full_name};
    my $fact = dump_write_table($entry->{module}, $entry->{table}, $full_name, $entry->{hash});
    $fact->{effective_write_proc} = effective_write_code_fact(
        $entry->{hash}, 'WRITE_PROC', "${full_name}::WRITE_PROC",
        $EXIFTOOL_LIB_ABS, $write_autoload_status);
    $fact->{effective_check_proc} = effective_write_code_fact(
        $entry->{hash}, 'CHECK_PROC', "${full_name}::CHECK_PROC",
        $EXIFTOOL_LIB_ABS, $write_autoload_status);
    $native_write_tables{$entry->{module}}{$entry->{table}} = $fact;
}

# Hydration above may load Writer.pl.  Snapshot the helper package bindings
# only now, after all selected modules and any permitted writer autoloads have
# settled.  This does not alter the read projection or route a native writer.
my $write_helper_status = hydrate_write_helpers();
my $native_write_helpers = native_write_helper_facts($EXIFTOOL_LIB_ABS, $write_helper_status);

# The child captures byte-order state without changing this table-walking
# process. These facts prove that the final loaded CODE refs are the same ones
# the child observed; a later module override makes the contract unresolved.
my $unsigned_reader_contract = OxiDex::NativeReaderContract::finalise_loaded_contract(
    OxiDex::NativeReaderContract::capture_isolated_contract(
        $^X, File::Spec->catfile(dirname(abs_path($0) // $0),
            'dump_binary_reader_contract.pl'), $EXIFTOOL_LIB),
    $EXIFTOOL_LIB_ABS);

# ->utf8 makes the encoder emit UTF-8 *bytes*.  Without it JSON::PP returns a
# character string and print() downgrades anything under U+0100 to a raw
# Latin-1 byte -- which is exactly how a copyright sign in a Notes field ends
# up as an invalid 0xA9 in the output.
my $json = JSON::PP->new->utf8->canonical->pretty;
print $json->encode({
    exiftool_version => $Image::ExifTool::VERSION,
    modules_ok       => $ok,
    modules_failed   => $failed,
    modules          => \%out,
    # Facts only: no reader or writer consumes this sidecar yet.  Keeping it
    # separate prevents write-only properties from becoming accidental read
    # admission or changing the existing module/table/tag projection.
    native_write_autoload => $write_autoload_router_status,
    native_write_tables => \%native_write_tables,
    native_write_helpers => $native_write_helpers,
    subdirectory_validate_functions => \%subdirectory_validate_functions,
    native_reader_contracts => { unsigned16 => $unsigned_reader_contract },
});
