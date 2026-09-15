#!/usr/bin/env perl
# Capture authenticated native FindTagInfo candidates for generated rows.
use strict;
use warnings;
use B ();
use B::Deparse;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use File::Spec ();
use JSON::PP;
use Scalar::Util qw(refaddr);

my ($lib, $input_path) = @ARGV;
die "usage: setnewvalue_address_probe.pl EXIFTOOL_LIB probe-input.json\n" unless $lib && $input_path;
my $lib_abs = abs_path($lib) or die "invalid ExifTool lib: $lib\n";

# Must be first: unshift(@INC) cannot authenticate a package already loaded
# from main, Writer, TagLookup, or a transitive ambient path.
for my $inc (sort keys %INC) {
    next unless $inc eq 'Image/ExifTool.pm' || $inc =~ m{^Image/ExifTool/};
    die "ExifTool was preloaded before selected-library guard: $inc => $INC{$inc}\n";
}
unshift @INC, $lib_abs;
require Image::ExifTool;
require 'Image/ExifTool/Writer.pl';

sub read_raw {
    my ($path) = @_;
    open(my $fh, '<:raw', $path) or die "read $path: $!\n";
    local $/;
    my $bytes = <$fh>;
    close($fh) or die "close $path: $!\n";
    return $bytes;
}

sub relative_selected_file {
    my ($path) = @_;
    my $abs = abs_path($path);
    die "unreadable selected source: $path\n" unless defined($abs) && -f $abs;
    my $prefix = $lib_abs . '/';
    die "loaded source is outside selected library: $abs\n" unless index($abs, $prefix) == 0;
    return File::Spec->abs2rel($abs, $lib_abs);
}

sub loaded_closure {
    my @modules;
    for my $inc (sort keys %INC) {
        next unless $inc eq 'Image/ExifTool.pm' || $inc =~ m{^Image/ExifTool/};
        my $source_file = relative_selected_file($INC{$inc});
        push @modules, { inc => $inc, source_file => $source_file,
                         source_sha256 => sha256_hex(read_raw($INC{$inc})) };
    }
    die "selected ExifTool closure is empty\n" unless @modules;
    my $encoded = JSON::PP->new->canonical->utf8->encode(\@modules);
    return { sha256 => sha256_hex($encoded), modules => \@modules };
}

sub helper_fact {
    my ($binding) = @_;
    no strict 'refs';
    my $cv = *{$binding}{CODE} or die "native helper is unavailable: $binding\n";
    my $b = B::svref_2object($cv);
    my $gv = $b->GV;
    my $actual = ($gv->STASH->NAME // '') . '::' . ($gv->NAME // '');
    die "native helper binding was rebound: $binding => $actual\n" unless $actual eq $binding;
    my $body = B::Deparse->new('-p', '-sC')->coderef2text($cv);
    my $file = relative_selected_file($b->FILE);
    my $abs = File::Spec->catfile($lib_abs, $file);
    return { requested_binding => $binding, actual_name => $actual,
             source_file => $file, source_sha256 => sha256_hex(read_raw($abs)),
             body_sha256 => sha256_hex($body) };
}

my $input = decode_json(read_raw($input_path));
die "probe input is not an object\n" unless ref($input) eq 'HASH';
die "probe input schema is unsupported\n"
    unless $input->{schema} eq 'native_setnewvalue_address_probe_input_v3';
my $capture = $input->{capture};
my $rows = $input->{rows};
my $query_names = $input->{query_names};
die "probe capture is not an object\n" unless ref($capture) eq 'HASH';
die "probe rows is not an array\n" unless ref($rows) eq 'ARRAY';
die "probe query names are not an array\n" unless ref($query_names) eq 'ARRAY';

my $canonical = JSON::PP->new->canonical->utf8;
my $rows_digest = sha256_hex($canonical->encode($rows));
die "probe rows digest disagrees with capture\n"
    unless defined($capture->{source_rows_sha256}) && $capture->{source_rows_sha256} eq $rows_digest;
for my $name (@$query_names) {
    die "probe query name is not text\n" unless defined($name) && !ref($name);
}
my @query_names = sort map { lc($_) } @$query_names;
my %seen_name;
@query_names = grep { !$seen_name{$_}++ } @query_names;
my $query_digest = sha256_hex($canonical->encode(\@query_names));
die "probe query-name digest disagrees with capture\n"
    unless defined($capture->{query_names_sha256}) && $capture->{query_names_sha256} eq $query_digest;

my $context = $capture->{native_capture_context};
die "native capture context is not an object\n" unless ref($context) eq 'HASH';
my %actual_context = (
    selected_library => $lib_abs,
    perl_path => (abs_path($^X) // $^X),
    perl_version => "$]",
    exiftool_version => "$Image::ExifTool::VERSION",
);
for my $key (sort keys %actual_context) {
    die "selected runtime $key differs from dump capture\n"
        unless defined($context->{$key}) && $context->{$key} eq $actual_context{$key};
}
my $captured_closure = $context->{loaded_closure};
die "dump capture closure is not an object\n" unless ref($captured_closure) eq 'HASH';
die "dump capture closure modules are not an array\n" unless ref($captured_closure->{modules}) eq 'ARRAY';
die "dump capture closure digest is invalid\n"
    unless defined($captured_closure->{sha256}) && $captured_closure->{sha256} =~ /^[0-9a-f]{64}$/;
die "dump capture closure digest does not match its modules\n"
    unless sha256_hex($canonical->encode($captured_closure->{modules})) eq $captured_closure->{sha256};
my %captured_by_inc;
for my $module (@{$captured_closure->{modules}}) {
    die "dump capture closure member is not an object\n" unless ref($module) eq 'HASH';
    for my $key (qw(inc source_file source_sha256)) {
        die "dump capture closure member is malformed\n" unless defined($module->{$key}) && !ref($module->{$key});
    }
    die "dump capture closure has duplicate module binding: $module->{inc}\n"
        if exists $captured_by_inc{$module->{inc}};
    $captured_by_inc{$module->{inc}} = $module;
}

# Replay the authenticated loaded source closure before deparsing helpers.
# B::Deparse consults prototypes of already-loaded callees, so deparsing a
# settled dump in a smaller process can change its text without changing code.
# Check every source file before require; never execute an unverified closure.
for my $inc (sort keys %captured_by_inc) {
    my $member = $captured_by_inc{$inc};
    die "unsupported captured module binding: $inc\n"
        unless $inc =~ m{\AImage/ExifTool(?:\.pm|/[A-Za-z0-9_/]+\.(?:pm|pl))\z};
    die "captured module source identity differs: $inc\n"
        unless $member->{source_file} eq $inc;
    my $file = File::Spec->catfile($lib_abs, $inc);
    die "captured module source escaped selected library: $inc\n"
        unless relative_selected_file($file) eq $inc;
    die "dump capture module source differs from selected library: $inc\n"
        unless sha256_hex(read_raw($file)) eq $member->{source_sha256};
}
for my $inc (sort keys %captured_by_inc) {
    require $inc;
    die "captured module resolved outside selected source: $inc\n"
        unless relative_selected_file($INC{$inc}) eq $inc;
}
my $runtime_find = helper_fact('Image::ExifTool::TagLookup::FindTagInfo');
my $runtime_setnew = helper_fact('Image::ExifTool::SetNewValue');
for my $pair ([$runtime_find, $capture->{find_tag_info}, 'FindTagInfo'],
              [$runtime_setnew, $capture->{set_new_value}, 'SetNewValue']) {
    my ($live, $saved, $label) = @$pair;
    die "$label capture is not an object\n" unless ref($saved) eq 'HASH';
    die "$label identity differs from supplied dump capture\n"
        unless $canonical->encode($live) eq $canonical->encode($saved);
}

my %selected;
for my $row (@$rows) {
    die "row is not an object\n" unless ref($row) eq 'HASH';
    for my $key (qw(module table full_name raw_id name)) {
        die "row $key is not text\n" unless defined($row->{$key}) && !ref($row->{$key});
    }
    my $table = Image::ExifTool::GetTagTable($row->{full_name})
        or die "cannot load selected table $row->{full_name}\n";
    $selected{refaddr($table)} = { map { $_ => $row->{$_} } qw(module table full_name) };
}

# `FindTagInfo` returns table references, not a durable table spelling.  The
# old probe recorded an unselected reference merely as "external", which
# loses the distinction between two physical EXIF/IFD0 fields (notably
# Exif::Main and PanasonicRaw::Main Artist).  Resolve every selected native
# table reference through TagLookup's authoritative table list before we
# inspect candidates.  This is intentionally not a group/name heuristic.
#
# Loading a previously unseen candidate table changes the native closure; the
# closure equality check below then refuses an old dump capture.  A successful
# regeneration must therefore have warmed and captured the same table.
my $lookup_source = read_raw(File::Spec->catfile($lib_abs, 'Image/ExifTool/TagLookup.pm'));
$lookup_source =~ /my\s+\@tableList\s*=\s*\(\n(.*?)^\);/ms
    or die "TagLookup tableList source is unsupported\n";
my @native_table_names = ($1 =~ /^\s*'([^']+)'\s*,?\s*(?:#.*)?$/mg);
die "TagLookup tableList source is empty\n" unless @native_table_names;
my %native_table_identity;
for my $full (@native_table_names) {
    next unless defined $full && $full =~ /^Image::ExifTool::([^:]+)::([^:]+)$/;
    my $table = Image::ExifTool::GetTagTable($full) or next;
    $native_table_identity{refaddr($table)} = {
        module => $1, table => $2, full_name => $full,
    };
}
my $et = Image::ExifTool->new;
my %queries;
for my $lower (@query_names) {
    my $name = $lower;
    my @candidates;
    for my $info (Image::ExifTool::TagLookup::FindTagInfo($name)) {
        my $table = $info->{Table};
        # Preserve table-local write controls with the candidate identity.  A
        # same numeric id and group is not an alias when `Permanent` or the
        # selected table's write controls differ.
        my %candidate = (name => "$info->{Name}", raw_id => "$info->{TagID}",
                         writable => (defined $info->{Writable} ? "$info->{Writable}" : undef),
                         permanent => ($info->{Permanent} ? JSON::PP::true : JSON::PP::false),
                         write_group => (defined $info->{WriteGroup} ? "$info->{WriteGroup}" : undef));
        if (my $identity = $native_table_identity{refaddr($table)}) {
            @candidate{qw(module table full_name)} = @{$identity}{qw(module table full_name)};
        } else {
            $candidate{external_table} = (ref($table) eq 'HASH' && defined($table->{TABLE_NAME}))
                ? "$table->{TABLE_NAME}" : 'unidentified';
        }
        my @groups = $et->GetGroup($info);
        $candidate{groups} = { map { $_ => "$groups[$_]" } grep { defined $groups[$_] } 0 .. $#groups };
        push @candidates, \%candidate;
    }
    $queries{$lower} = { query => $name, candidates => \@candidates };
}
my $closure = loaded_closure();
for my $module (@{$closure->{modules}}) {
    my $captured = $captured_by_inc{$module->{inc}};
    die "loaded module is missing from dump capture: $module->{inc}\n" unless $captured;
    die "loaded module differs from dump capture: $module->{inc}\n"
        unless $canonical->encode($module) eq $canonical->encode($captured);
}
print JSON::PP->new->canonical->utf8->encode({
    schema => 'native_setnewvalue_addressing_v3', capture => $capture,
    runtime => { %actual_context, loaded_closure => $closure,
                 helpers => { find_tag_info => $runtime_find, set_new_value => $runtime_setnew } },
    queries => \%queries,
}), "\n";
