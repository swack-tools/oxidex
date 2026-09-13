#!/usr/bin/env perl
# Native JSONL oracle for source-selected word-directory PROCESS_PROC callbacks.
#
# This deliberately has no knowledge of a particular camera, table name, or
# compiler grammar.  It loads the requested native table, calls its actual
# PROCESS_PROC with a bounded DirInfo, and records the callbacks the processor
# makes.  Python callers own timeouts; stdout stays one canonical JSON object
# per request so an error can never be mistaken for a passing trace.

use strict;
use warnings;
use B ();
use B::Deparse ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Encode ();
use File::Spec ();
use Getopt::Long qw(GetOptions);
use JSON::PP ();
use Scalar::Util qw(looks_like_number refaddr);

my (%opt, @roots);
GetOptions(
    'lib=s'          => \$opt{lib},
    'fallback-lib=s' => \$opt{fallback_lib},
) or die "usage: $0 --lib RELATIVE_LIB [--fallback-lib RELATIVE_LIB]\n";
die "usage: $0 --lib RELATIVE_LIB [--fallback-lib RELATIVE_LIB]\n"
    unless defined $opt{lib};

sub relative_root {
    my ($label, $path) = @_;
    die "$label must be a relative directory\n"
        if !defined($path) || File::Spec->file_name_is_absolute($path)
            || $path =~ m{(?:^|[\\/])\.\.(?:[\\/]|$)};
    my $absolute = abs_path($path);
    die "$label does not name a directory\n" unless defined($absolute) && -d $absolute;
    return { label => $label, absolute => $absolute };
}

push @roots, relative_root('lib', $opt{lib});
push @roots, relative_root('fallback_lib', $opt{fallback_lib}) if defined $opt{fallback_lib};
# Keep the copied/selected source first. A fallback exists only to satisfy its
# imports; reversing this order would silently execute the pinned module and
# make a copied-source mutation appear to pass.
unshift @INC, map { $_->{absolute} } @roots;

sub json {
    return JSON::PP->new->canonical->utf8;
}

sub code_name {
    my ($cv) = @_;
    my $b = eval { B::svref_2object($cv) } or return undef;
    return undef unless $b->isa('B::CV');
    my $gv = eval { $b->GV } or return undef;
    return undef if ref($gv) eq 'B::SPECIAL';
    my $stash = eval { $gv->STASH->NAME } // '';
    my $name = eval { $gv->NAME } // '';
    return undef unless length $name;
    return length($stash) ? "$stash\::$name" : $name;
}

sub source_root_for {
    my ($absolute) = @_;
    for my $root (@roots) {
        my $prefix = $root->{absolute} . '/';
        return $root if index($absolute, $prefix) == 0;
    }
    return undef;
}

sub code_fact {
    my ($cv, $requested) = @_;
    return { resolved => JSON::PP::false, requested => $requested, reason => 'not_code' }
        unless ref($cv) eq 'CODE';
    my $name = code_name($cv) // $requested;
    my $source = eval { B::svref_2object($cv)->FILE };
    my $absolute = defined($source) ? abs_path($source) : undef;
    my $root = defined($absolute) ? source_root_for($absolute) : undef;
    return { resolved => JSON::PP::false, requested => $requested, name => $name,
             reason => 'source_outside_selected_libs' }
        unless defined($root) && -f $absolute;
    open(my $fh, '<:raw', $absolute)
        or return { resolved => JSON::PP::false, requested => $requested, name => $name,
                    reason => 'source_unreadable' };
    local $/;
    my $bytes = <$fh>;
    close($fh);
    my $deparse = eval { B::Deparse->new('-p', '-sC')->coderef2text($cv) };
    return { resolved => JSON::PP::false, requested => $requested, name => $name,
             reason => 'deparse_unavailable' }
        unless defined $deparse;
    return {
        resolved => JSON::PP::true,
        requested => $requested,
        name => $name,
        source_root => $root->{label},
        source_file => File::Spec->abs2rel($absolute, $root->{absolute}),
        source_sha256 => sha256_hex($bytes),
        source_body_sha256 => sha256_hex(Encode::encode_utf8($deparse)),
    };
}

sub scalar_fact {
    my ($value) = @_;
    return { defined => JSON::PP::false } unless defined $value;
    return { defined => JSON::PP::true, reference => ref($value) } if ref($value);
    my $fact = { defined => JSON::PP::true, string => "$value" };
    $fact->{numeric} = 0 + $value if looks_like_number($value);
    return $fact;
}

{
    package OxiDex::WordProcessorProbe;
    use strict;
    use warnings;
    use Scalar::Util qw(refaddr);
    our @ISA = ('Image::ExifTool');

    sub new_probe {
        my ($class, $table_ref, $members, $verbose) = @_;
        my $self = bless {
            _probe_table_address => refaddr($table_ref),
            _probe_calls => [], _probe_warnings => [], _probe_verbose_dirs => [],
            _probe_unexpected => [], _probe_verbose => $verbose ? 1 : 0,
        }, $class;
        $self->{$_} = $members->{$_} for keys %$members;
        return $self;
    }

    sub Options {
        my ($self, $name) = @_;
        push @{ $self->{_probe_calls} }, { method => 'Options', argument => "$name" };
        return $self->{_probe_verbose} if $name eq 'Verbose';
        push @{ $self->{_probe_unexpected} }, "Options:$name";
        return undef;
    }

    sub Warn {
        my ($self, @args) = @_;
        push @{ $self->{_probe_calls} }, { method => 'Warn' };
        push @{ $self->{_probe_warnings} }, [ map { main::scalar_fact($_) } @args ];
        return undef;
    }

    sub VerboseDir {
        my ($self, @args) = @_;
        push @{ $self->{_probe_calls} }, { method => 'VerboseDir' };
        push @{ $self->{_probe_verbose_dirs} }, [ map { main::scalar_fact($_) } @args ];
        return undef;
    }

    sub HandleTag {
        my ($self, $table, $id, $value, @args) = @_;
        push @{ $self->{_probe_calls} }, { method => 'HandleTag' };
        my @raw = (
            { table_reference => refaddr($table) == $self->{_probe_table_address}
                ? 'selected_table' : 'unexpected_table' },
            main::scalar_fact($id), main::scalar_fact($value),
            map { main::scalar_fact($_) } @args,
        );
        push @{ $self->{_probe_unexpected} }, 'HandleTag:odd_named_arguments'
            if @args % 2;
        my %named;
        while (@args >= 2) {
            my ($name, $arg) = splice(@args, 0, 2);
            if (!defined($name) || ref($name)) {
                push @{ $self->{_probe_unexpected} }, 'HandleTag:non_scalar_name';
                next;
            }
            $named{"$name"} = main::scalar_fact($arg);
        }
        push @{ $self->{_probe_unexpected} }, 'HandleTag:unexpected_table'
            if $raw[0]{table_reference} ne 'selected_table';
        push @{ $self->{_probe_handle_tags} }, {
            raw_args => \@raw,
            raw_id => main::scalar_fact($id),
            value => main::scalar_fact($value),
            named_arguments => \%named,
            index => $named{Index}, format => $named{Format},
            count => $named{Count}, size => $named{Size},
        };
        return 1;
    }
}

sub request_error {
    my ($request, $kind, $message) = @_;
    return {
        protocol => 'oxidex.word_processor.v1', ok => JSON::PP::false,
        request => $request, error => { kind => $kind, message => $message },
    };
}

sub canonical_module {
    my ($module) = @_;
    return undef unless defined($module) && !ref($module);
    $module = "Image::ExifTool::$module" unless $module =~ /^Image::ExifTool::/;
    return undef unless $module =~ /^Image::ExifTool(?:::[A-Za-z_]\w*)+$/;
    return $module;
}

sub load_selected_table {
    my ($module, $table) = @_;
    return (undef, 'invalid_module') unless defined $module;
    return (undef, 'invalid_table') unless defined($table) && !ref($table) && $table =~ /^[A-Za-z_]\w*$/;
    (my $path = $module) =~ s!::!/!g;
    my $loaded = eval { require "$path.pm"; 1 };
    return (undef, "module_load_failed:$@") unless $loaded;
    no strict 'refs';
    my $table_ref = *{"${module}::${table}"}{HASH};
    return (undef, 'table_unavailable') unless ref($table_ref) eq 'HASH';
    my $process = $table_ref->{PROCESS_PROC};
    return (undef, 'process_proc_unavailable') unless ref($process) eq 'CODE';
    return ({ table_ref => $table_ref, process => $process }, undef);
}

sub valid_case {
    my ($case) = @_;
    return (undef, 'case_must_be_object') unless ref($case) eq 'HASH';
    return (undef, 'invalid_byte_order') unless ($case->{byte_order} // '') =~ /^(?:II|MM)$/;
    return (undef, 'invalid_data_hex') unless defined($case->{data_hex})
        && !ref($case->{data_hex}) && $case->{data_hex} =~ /\A(?:[0-9a-fA-F]{2})*\z/;
    my $data = pack('H*', $case->{data_hex});
    return (undef, 'input_too_large') if length($data) > 1_048_576;
    for my $key (qw(dir_start dir_len)) {
        return (undef, "invalid_$key") unless defined($case->{$key}) && !ref($case->{$key})
            && $case->{$key} =~ /\A(?:0|[1-9][0-9]*)\z/;
    }
    return (undef, 'directory_outside_data')
        if $case->{dir_start} + $case->{dir_len} > length($data);
    my $members = $case->{members} // {};
    return (undef, 'members_must_be_object') unless ref($members) eq 'HASH';
    for my $key (keys %$members) {
        return (undef, 'invalid_member_name') unless $key =~ /^[A-Za-z_]\w*$/;
        return (undef, 'invalid_member_value') if ref($members->{$key});
    }
    return ({ data => $data, members => $members }, undef);
}

sub process_request {
    my ($request) = @_;
    return request_error(undef, 'request_must_be_object', 'JSONL item is not an object')
        unless ref($request) eq 'HASH';
    return request_error($request, 'protocol', 'expected oxidex.word_processor.v1')
        unless ($request->{protocol} // '') eq 'oxidex.word_processor.v1';
    my $module = canonical_module($request->{module});
    return request_error($request, 'module', 'expected Image::ExifTool module name') unless defined $module;
    my ($selected, $selection_error) = load_selected_table($module, $request->{table});
    return request_error($request, 'selection', $selection_error) unless defined $selected;
    my ($case, $case_error) = valid_case($request->{case});
    return request_error($request, 'case', $case_error) unless defined $case;

    my $table = $request->{table};
    my $process_fact = code_fact($selected->{process}, "${module}::${table}::PROCESS_PROC");
    return request_error($request, 'process_fact', $process_fact->{reason}) unless $process_fact->{resolved};
    my ($owner) = $process_fact->{name} =~ /\A(.+)::[^:]+\z/;
    my $binding_name = "${owner}::Get16u";
    no strict 'refs';
    my $reader_fact = code_fact(*{$binding_name}{CODE}, $binding_name);
    return request_error($request, 'reader_binding', $reader_fact->{reason}) unless $reader_fact->{resolved};

    my $before = eval { Image::ExifTool::GetByteOrder() };
    my ($set_error, $restore_error, $returned, @perl_warnings, $active_order);
    my $probe = OxiDex::WordProcessorProbe->new_probe(
        $selected->{table_ref}, $case->{members}, $request->{case}{verbose},
    );
    my $set_ok = eval { Image::ExifTool::SetByteOrder($request->{case}{byte_order}); 1 };
    $set_error = $@ unless $set_ok;
    if ($set_ok) {
        $active_order = eval { Image::ExifTool::GetByteOrder() };
        local $SIG{__WARN__} = sub { push @perl_warnings, "$_[0]" };
        # Package-qualified object callbacks must never silently fall through
        # to Image::ExifTool methods. Normal `$et->...` dispatch uses the probe
        # overrides above; a direct base-class callback becomes explicit error.
        no warnings qw(redefine once);
        local *Image::ExifTool::HandleTag = sub { die "unexpected package-qualified HandleTag callback" };
        local *Image::ExifTool::Warn = sub { die "unexpected package-qualified Warn callback" };
        local *Image::ExifTool::VerboseDir = sub { die "unexpected package-qualified VerboseDir callback" };
        local *Image::ExifTool::Options = sub { die "unexpected package-qualified Options callback" };
        my $ok = eval {
            my $data = $case->{data};
            $returned = $selected->{process}->(
                $probe,
                { DataPt => \$data, DirStart => 0 + $request->{case}{dir_start}, DirLen => 0 + $request->{case}{dir_len} },
                $selected->{table_ref},
            );
            1;
        };
        $set_error = $@ unless $ok;
    }
    my $restore_ok = eval { Image::ExifTool::SetByteOrder($before); 1 };
    $restore_error = $@ unless $restore_ok;
    my $error = $set_error || $restore_error;
    return {
        protocol => 'oxidex.word_processor.v1', ok => $error ? JSON::PP::false : JSON::PP::true,
        request => { module => $module, table => $request->{table}, case => $request->{case}{name} },
        selection => { process => $process_fact, reader_binding => $reader_fact },
        byte_order => { before => $before, requested => $request->{case}{byte_order}, active => $active_order,
                        restored => eval { Image::ExifTool::GetByteOrder() }, restore_error => $restore_error || undef },
        returned => scalar_fact($returned),
        warnings => $probe->{_probe_warnings}, verbose_dirs => $probe->{_probe_verbose_dirs},
        handle_tags => $probe->{_probe_handle_tags} // [], method_calls => $probe->{_probe_calls},
        unexpected_side_effects => $probe->{_probe_unexpected}, perl_warnings => \@perl_warnings,
        error => $error || undef,
    };
}

while (my $line = <STDIN>) {
    next if $line =~ /^\s*$/;
    my $request = eval { JSON::PP::decode_json($line) };
    my $response = $@ ? request_error(undef, 'invalid_json', $@) : process_request($request);
    print json()->encode($response), "\n";
}
