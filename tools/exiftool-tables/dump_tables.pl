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
my @TAG_KEYS = qw(
    Name Description Format Writable Count Groups Notes Mask Condition
    PrintConv ValueConv RawConv PrintConvInv ValueConvInv Hook
    SubDirectory Flags Unknown Hidden Avoid Binary Protected List
    Priority ByteOrder DataMember RelatedTag SeparateTable PrintHex
    Base Offset ChangeBase
    Require Desire Inhibit
);

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
        return { __perl => 'CODE', __opaque => 1, defined $n ? (__name => $n) : () };
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
    if ($r eq 'CODE')  { return { kind => 'code', expr => undef } }
    if ($r eq 'ARRAY') { return { kind => 'list', items => scrub($v) } }
    if (!$r)           { return { kind => 'expr', expr => to_text($v) } }
    return { kind => 'other', dump => scrub($v) };
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
    my ($module) = @_;
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
            $tags{$k} = dump_tag_entry($hash->{$k});
        }
        my %meta;
        for my $k (grep { $TABLE_META{$_} } @keys) {
            $meta{$k} = scrub($hash->{$k});
        }

        $tables{$sym} = {
            full_name => "${pkg}::${sym}",
            meta      => \%meta,
            tags      => \%tags,
            tag_count => scalar(keys %tags),
        };
    }

    return {
        module     => $module,
        package    => $pkg,
        tables     => \%tables,
        table_count=> scalar(keys %tables),
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
my ($ok, $failed) = (0, 0);
for my $m (@modules) {
    my $r = dump_module($m);
    if ($r->{error}) {
        $failed++;
        warn "SKIP $m: $r->{error}";
        next;
    }
    next unless $r->{table_count};
    $out{$m} = $r;
    $ok++;
}

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
});
