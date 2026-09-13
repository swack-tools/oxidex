#!/usr/bin/env perl
# Native JSONL oracle for source-selected serial-data PROCESS_PROC callbacks.
#
# This helper deliberately has no knowledge of Canon IDs, table names, or a
# compiler grammar. Each request chooses the native module/table. It invokes
# the table's actual PROCESS_PROC with bounded bytes and records only object
# callbacks ExifTool actually makes; direct package ReadValue calls are named
# as unobserved rather than simulated.

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
# A copied source tree must win over fallback imports; otherwise a copied
# mutation could silently execute the pinned module instead.
unshift @INC, map { $_->{absolute} } @roots;

sub json { JSON::PP->new->canonical->utf8 }

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

sub tag_info_fact {
    my ($info) = @_;
    return { defined => JSON::PP::false } unless ref($info) eq 'HASH';
    my %fact = (defined => JSON::PP::true);
    for my $key (qw(Name Format Unknown)) {
        $fact{lc $key} = scalar_fact($info->{$key}) if exists $info->{$key};
    }
    # Conditions may be CODE/string/ref and are not interpreted by this probe.
    $fact{condition_kind} = ref($info->{Condition}) || 'scalar'
        if exists $info->{Condition};
    return \%fact;
}

{
    package OxiDex::SerialProcessorProbe;
    use strict;
    use warnings;
    use Scalar::Util qw(refaddr);
    our @ISA = ('Image::ExifTool');

    sub new_probe {
        my ($class, $table_ref, $members, $verbose, $unknown) = @_;
        my $self = bless {
            _probe_table_address => refaddr($table_ref),
            _probe_calls => [], _probe_warnings => [], _probe_verbose_dirs => [],
            _probe_verbose_info => [], _probe_get_tag_info => [], _probe_found_tags => [],
            _probe_options => { Verbose => $verbose ? 1 : 0, Unknown => $unknown ? 1 : 0 },
            OPTIONS => { Verbose => $verbose ? 1 : 0, Unknown => $unknown ? 1 : 0 },
        }, $class;
        $self->{$_} = $members->{$_} for keys %$members;
        return $self;
    }

    sub Options {
        my ($self, @args) = @_;
        if (@args == 1) {
            my ($name) = @args;
            push @{ $self->{_probe_calls} }, { method => 'Options', mode => 'get', argument => "$name" };
            return $self->{_probe_options}{$name};
        }
        if (@args == 2) {
            my ($name, $value) = @args;
            my $before = $self->{_probe_options}{$name};
            push @{ $self->{_probe_calls} }, {
                method => 'Options', mode => 'set', argument => "$name",
                before => main::scalar_fact($before), after => main::scalar_fact($value),
            };
            $self->{_probe_options}{$name} = $value;
            $self->{OPTIONS}{$name} = $value;
            return $before;
        }
        die 'unexpected Options arity';
    }

    sub GetTagInfo {
        my ($self, $table, $index, @args) = @_;
        my $info = $self->SUPER::GetTagInfo($table, $index, @args);
        push @{ $self->{_probe_calls} }, { method => 'GetTagInfo' };
        push @{ $self->{_probe_get_tag_info} }, {
            table_reference => refaddr($table) == $self->{_probe_table_address}
                ? 'selected_table' : 'unexpected_table',
            index => main::scalar_fact($index),
            result => main::tag_info_fact($info),
            tag_info_reference => ref($info) eq 'HASH' ? refaddr($info) : undef,
        };
        return $info;
    }

    sub FoundTag {
        my ($self, $info, $value, @args) = @_;
        push @{ $self->{_probe_calls} }, { method => 'FoundTag' };
        push @{ $self->{_probe_found_tags} }, {
            tag_info => main::tag_info_fact($info),
            tag_info_reference => ref($info) eq 'HASH' ? refaddr($info) : undef,
            value => main::scalar_fact($value),
            extra_arguments => [ map { main::scalar_fact($_) } @args ],
        };
        return 1;
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

    sub VerboseInfo {
        my ($self, $index, $info, @args) = @_;
        push @{ $self->{_probe_calls} }, { method => 'VerboseInfo' };
        my %named;
        while (@args >= 2) {
            my ($name, $value) = splice(@args, 0, 2);
            $named{"$name"} = main::scalar_fact($value)
                if defined($name) && !ref($name);
        }
        push @{ $self->{_probe_verbose_info} }, {
            index => main::scalar_fact($index), tag_info => main::tag_info_fact($info),
            named_arguments => \%named,
        };
        return undef;
    }
}

sub request_error {
    my ($request, $kind, $message) = @_;
    return {
        protocol => 'oxidex.serial_processor.v1', ok => JSON::PP::false,
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
    for my $key (qw(data_pos base)) {
        next unless exists $case->{$key};
        return (undef, "invalid_$key") unless defined($case->{$key}) && !ref($case->{$key})
            && $case->{$key} =~ /\A(?:0|[1-9][0-9]*)\z/;
    }
    for my $key (qw(verbose unknown)) {
        next unless exists $case->{$key};
        return (undef, "invalid_$key") unless !ref($case->{$key})
            && ($case->{$key} eq '0' || $case->{$key} eq '1');
    }
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
    return request_error($request, 'protocol', 'expected oxidex.serial_processor.v1')
        unless ($request->{protocol} // '') eq 'oxidex.serial_processor.v1';
    my $module = canonical_module($request->{module});
    return request_error($request, 'module', 'expected Image::ExifTool module name') unless defined $module;
    my ($selected, $selection_error) = load_selected_table($module, $request->{table});
    return request_error($request, 'selection', $selection_error) unless defined $selected;
    my ($case, $case_error) = valid_case($request->{case});
    return request_error($request, 'case', $case_error) unless defined $case;

    my $table = $request->{table};
    my $process_fact = code_fact($selected->{process}, "${module}::${table}::PROCESS_PROC");
    return request_error($request, 'process_fact', $process_fact->{reason}) unless $process_fact->{resolved};

    my $before = eval { Image::ExifTool::GetByteOrder() };
    my ($run_error, $restore_error, $returned, @perl_warnings, $active_order);
    my $probe = OxiDex::SerialProcessorProbe->new_probe(
        $selected->{table_ref}, $case->{members}, $request->{case}{verbose}, $request->{case}{unknown},
    );
    my $set_ok = eval { Image::ExifTool::SetByteOrder($request->{case}{byte_order}); 1 };
    $run_error = $@ unless $set_ok;
    if ($set_ok) {
        $active_order = eval { Image::ExifTool::GetByteOrder() };
        local $SIG{__WARN__} = sub { push @perl_warnings, "$_[0]" };
        # The selected processor should dispatch through this object's methods.
        # A package-qualified object callback is a probe error, never a pass.
        no warnings qw(redefine once);
        local *Image::ExifTool::FoundTag = sub { die 'unexpected package-qualified FoundTag callback' };
        local *Image::ExifTool::Warn = sub { die 'unexpected package-qualified Warn callback' };
        local *Image::ExifTool::Options = sub { die 'unexpected package-qualified Options callback' };
        my $ok = eval {
            my $data = $case->{data};
            $returned = $selected->{process}->(
                $probe,
                {
                    DataPt => \$data,
                    DirStart => 0 + $request->{case}{dir_start},
                    DirLen => 0 + $request->{case}{dir_len},
                    (exists($request->{case}{data_pos}) ? (DataPos => 0 + $request->{case}{data_pos}) : ()),
                    (exists($request->{case}{base}) ? (Base => 0 + $request->{case}{base}) : ()),
                },
                $selected->{table_ref},
            );
            1;
        };
        $run_error = $@ unless $ok;
    }
    my $restore_ok = eval { Image::ExifTool::SetByteOrder($before); 1 };
    $restore_error = $@ unless $restore_ok;
    my $error = $run_error || $restore_error;
    return {
        protocol => 'oxidex.serial_processor.v1', ok => $error ? JSON::PP::false : JSON::PP::true,
        request => { module => $module, table => $request->{table}, case => $request->{case}{name} },
        selection => { process => $process_fact },
        byte_order => {
            before => $before, requested => $request->{case}{byte_order}, active => $active_order,
            restored => eval { Image::ExifTool::GetByteOrder() }, restore_error => $restore_error || undef,
        },
        returned => scalar_fact($returned),
        warnings => $probe->{_probe_warnings},
        verbose_dirs => $probe->{_probe_verbose_dirs},
        verbose_info => $probe->{_probe_verbose_info},
        get_tag_info => $probe->{_probe_get_tag_info},
        found_tags => $probe->{_probe_found_tags},
        method_calls => $probe->{_probe_calls},
        option_events => [ grep { $_->{method} eq 'Options' } @{ $probe->{_probe_calls} } ],
        option_state => {
            unknown_before => scalar_fact($request->{case}{unknown} ? 1 : 0),
            unknown_after => scalar_fact($probe->{_probe_options}{Unknown}),
            no_unknown_after => scalar_fact(exists($probe->{NO_UNKNOWN}) ? $probe->{NO_UNKNOWN} : undef),
        },
        observability => {
            get_tag_info => 'observed via object callback',
            found_tag => 'observed via object callback',
            verbose_info => 'observed only when native Options(Verbose) is true',
            read_value => 'unobserved: ProcessSerialData calls package ReadValue directly',
            dynamic_count_eval => 'unobserved: Perl eval occurs inside ProcessSerialData before verbose callback',
            nested_process_directory => 'unobserved: not intercepted by this native-only probe',
        },
        perl_warnings => \@perl_warnings,
        error => $error || undef,
    };
}

while (my $line = <STDIN>) {
    next if $line =~ /^\s*$/;
    my $request = eval { JSON::PP::decode_json($line) };
    my $response = $@ ? request_error(undef, 'invalid_json', $@) : process_request($request);
    print json()->encode($response), "\n";
}
