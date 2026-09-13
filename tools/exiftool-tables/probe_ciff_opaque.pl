#!/usr/bin/env perl
# Native JSONL observation probe for source-selected CIFF scalar entries.
use strict;
use warnings;
use B ();
use B::Deparse ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Getopt::Long qw(GetOptions);
use JSON::PP ();
use File::Spec ();

my $lib;
GetOptions('lib=s' => \$lib) or die "usage: $0 --lib RELATIVE_LIB\n";
die "usage: $0 --lib RELATIVE_LIB\n" unless defined $lib;
die "lib must be relative\n" if File::Spec->file_name_is_absolute($lib) || $lib =~ m{(?:^|/)\.\.(?:/|$)};
my $lib_abs = abs_path($lib);
die "lib must be a directory\n" unless defined($lib_abs) && -d $lib_abs;
unshift @INC, $lib_abs;
require Image::ExifTool;
require Image::ExifTool::CanonRaw;
require File::RandomAccess;

sub fact {
    my ($value) = @_;
    return { defined => JSON::PP::false } unless defined $value;
    return { defined => JSON::PP::true, reference => ref($value) } if ref $value;
    my $out = { defined => JSON::PP::true, string => "$value" };
    $out->{numeric} = 0 + $value if $value =~ /\A[+-]?(?:\d+\.?\d*|\.\d+)\z/;
    return $out;
}

sub bytes_fact {
    my ($value) = @_;
    $value = $$value if ref($value) eq 'SCALAR';
    return { defined => JSON::PP::false } unless defined $value;
    return { defined => JSON::PP::true, reference => ref($value) } if ref $value;
    return { defined => JSON::PP::true, length => length($value), hex => unpack('H*', $value) };
}

sub code_fact {
    my ($cv) = @_;
    return { resolved => JSON::PP::false, reason => 'not_code' } unless ref($cv) eq 'CODE';
    my $obj = eval { B::svref_2object($cv) } or return { resolved => JSON::PP::false, reason => 'no_cv' };
    my $gv = eval { $obj->GV } or return { resolved => JSON::PP::false, reason => 'no_gv' };
    my $name = eval { $gv->STASH->NAME . '::' . $gv->NAME } // '';
    my $file = eval { abs_path($obj->FILE) };
    return { resolved => JSON::PP::false, name => $name, reason => 'source_outside_lib' }
        unless defined($file) && index($file, "$lib_abs/") == 0 && -f $file;
    open my $fh, '<:raw', $file or return { resolved => JSON::PP::false, name => $name, reason => 'source_unreadable' };
    local $/; my $bytes = <$fh>; close $fh;
    my $body = eval { B::Deparse->new('-p','-sC')->coderef2text($cv) };
    return { resolved => JSON::PP::false, name => $name, reason => 'deparse_unavailable' } unless defined $body;
    return { resolved => JSON::PP::true, name => $name,
        source_file => File::Spec->abs2rel($file, $lib_abs), source_sha256 => sha256_hex($bytes),
        source_body_sha256 => sha256_hex($body) };
}

{
    package OxiDex::CiffOpaqueProbe;
    our @ISA = ('Image::ExifTool');
    sub new_probe {
        my ($class, $hash) = @_;
        my $self = $class->SUPER::new;
        $self->{OPTIONS}{Verbose} = 0;
        $self->{OPTIONS}{Binary} = 0;
        $self->{_found} = []; $self->{_warnings} = []; $self->{_hashes} = [];
        $self->{ImageDataHash} = 1 if $hash;
        return $self;
    }
    sub Warn {
        my ($self, @args) = @_;
        push @{$self->{_warnings}}, [map { main::fact($_) } @args];
        return undef;
    }
    sub FoundTag {
        my ($self, $tag_info, $value, @args) = @_;
        my $key = $self->SUPER::FoundTag($tag_info, $value, @args);
        my $stored = defined($key) ? $self->{VALUE}{$key} : undef;
        push @{$self->{_found}}, {
            name => main::fact(ref($tag_info) eq 'HASH' ? $tag_info->{Name} : $tag_info),
            key => main::fact($key), input_reference => ref($value) || undef,
            stored_reference => ref($stored) || undef,
            input_bytes => main::bytes_fact($value), stored_bytes => main::bytes_fact($stored),
            group2 => main::fact(ref($tag_info) eq 'HASH' ? $tag_info->{Groups}{2} : undef),
        };
        return $key;
    }
    sub ImageDataHash {
        my ($self, $raf, $size, $mode) = @_;
        push @{$self->{_hashes}}, { offset => main::fact($raf->Tell), size => main::fact($size), mode => main::fact($mode) };
        return 1;
    }
}

sub pack16 { $_[0] eq 'II' ? pack('v', $_[1]) : pack('n', $_[1]) }
sub pack32 { $_[0] eq 'II' ? pack('V', $_[1]) : pack('N', $_[1]) }

sub make_block {
    my ($order, $entries) = @_;
    my $dir = 4;
    my $directory_len = 2 + 10 * @$entries;
    my $next = $dir + $directory_len;
    $next = 64 if $next < 64;
    my @placed;
    for my $entry (@$entries) {
        next if exists $entry->{inline_hex};
        my $payload = pack('H*', $entry->{payload_hex});
        $entry->{_ptr} = defined($entry->{offset}) ? $entry->{offset} : $next;
        $entry->{_size} = length $payload;
        $next = $entry->{_ptr} + length($payload) if $entry->{_ptr} + length($payload) > $next;
        push @placed, [$entry->{_ptr}, $payload];
    }
    my $size = $next + 4;
    my $block = "\0" x $size;
    substr($block, $dir, 2) = pack16($order, scalar @$entries);
    for my $i (0 .. $#$entries) {
        my $entry = $entries->[$i];
        my $at = $dir + 2 + 10 * $i;
        substr($block, $at, 2) = pack16($order, $entry->{tag});
        if (exists $entry->{inline_hex}) {
            my $raw = pack('H*', $entry->{inline_hex});
            die 'inline_hex must contain eight bytes' unless length($raw) == 8;
            substr($block, $at + 2, 8) = $raw;
        } else {
            substr($block, $at + 2, 4) = pack32($order, $entry->{_size});
            substr($block, $at + 6, 4) = pack32($order, $entry->{_ptr});
        }
    }
    for my $placed (@placed) { substr($block, $placed->[0], length($placed->[1])) = $placed->[1]; }
    substr($block, $size - 4, 4) = pack32($order, $dir);
    return $block;
}

sub table_row_fact {
    my ($table, $tag) = @_;
    my $id = $tag & 0x3fff;
    my $row = $table->{$id};
    return { raw_id => $id, resolved => JSON::PP::false } unless ref($row) eq 'HASH';
    return {
        raw_id => $id, resolved => JSON::PP::true, name => fact($row->{Name}),
        format => fact($row->{Format}), binary => fact($row->{Binary}), raw_conv => fact($row->{RawConv}),
        group2 => fact($row->{Groups}{2}),
    };
}

sub run {
    my ($req) = @_;
    return { ok => JSON::PP::false, error => 'request_not_object' } unless ref($req) eq 'HASH';
    return { ok => JSON::PP::false, error => 'protocol' } unless ($req->{protocol} // '') eq 'oxidex.ciff_opaque.v1';
    my $order = $req->{byte_order} // '';
    return { ok => JSON::PP::false, error => 'byte_order' } unless $order =~ /^(?:II|MM)$/;
    return { ok => JSON::PP::false, error => 'entries' } unless ref($req->{entries}) eq 'ARRAY' && @{$req->{entries}};
    for my $entry (@{$req->{entries}}) {
        return { ok => JSON::PP::false, error => 'entry' } unless ref($entry) eq 'HASH' && defined($entry->{tag}) && $entry->{tag} =~ /^\d+$/;
        return { ok => JSON::PP::false, error => 'entry_payload' } unless (exists $entry->{inline_hex}) ^ (exists $entry->{payload_hex});
        my $hex = exists($entry->{inline_hex}) ? $entry->{inline_hex} : $entry->{payload_hex};
        return { ok => JSON::PP::false, error => 'entry_hex' } unless defined($hex) && $hex =~ /\A(?:[0-9a-fA-F]{2})*\z/;
    }
    my $before = Image::ExifTool::GetByteOrder();
    my $block = make_block($order, $req->{entries});
    my $raf = File::RandomAccess->new(\$block);
    Image::ExifTool::SetByteOrder($order);
    my $et = OxiDex::CiffOpaqueProbe->new_probe($req->{image_data_hash});
    my $table = Image::ExifTool::GetTagTable('Image::ExifTool::CanonRaw::Main');
    my ($returned, $error);
    my $ok = eval { $returned = Image::ExifTool::CanonRaw::ProcessCanonRaw($et, { RAF => $raf, DirStart => 0, DirLen => length($block), Nesting => 0, DirName => 'Main' }, $table); 1 };
    $error = $@ unless $ok;
    my $restore = eval { Image::ExifTool::SetByteOrder($before); 1 };
    $error ||= $@ unless $restore;
    return { protocol => 'oxidex.ciff_opaque.v1', ok => $error ? JSON::PP::false : JSON::PP::true,
        byte_order => { requested => $order, restored => Image::ExifTool::GetByteOrder() }, returned => fact($returned),
        selection => { process => code_fact(\&Image::ExifTool::CanonRaw::ProcessCanonRaw), validate_image => code_fact(\&Image::ExifTool::ValidateImage),
            rows => [ map { table_row_fact($table, $_->{tag}) } @{$req->{entries}} ] },
        found => $et->{_found}, warnings => $et->{_warnings}, image_data_hash => $et->{_hashes}, error => $error || undef };
}

while (my $line = <STDIN>) {
    next if $line =~ /^\s*$/;
    my $req = eval { JSON::PP::decode_json($line) };
    print JSON::PP->new->canonical->encode($@ ? { ok => JSON::PP::false, error => 'invalid_json' } : run($req)), "\n";
}
