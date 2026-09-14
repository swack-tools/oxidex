package OxiDex::Utf8PrimitiveContract;
use strict;
use warnings;
use utf8;
use B ();
use B::Deparse ();
use Config ();
use Digest::SHA qw(sha256_hex);
use DynaLoader ();
use JSON::PP ();
use File::Temp qw(tempfile);
use File::Basename qw(dirname);
use Cwd qw(abs_path);
use POSIX qw(WNOHANG);
use Time::HiRes qw(time sleep);

sub _file {
    my ($path) = @_;
    return { path => $path, resolved => JSON::PP::false } unless defined $path && -f $path;
    open my $file, '<:raw', $path or return { path => $path, resolved => JSON::PP::false };
    local $/;
    my $bytes = <$file>;
    close $file;
    return { path => $path, resolved => JSON::PP::true, sha256 => sha256_hex($bytes) };
}

sub _cv {
    my ($requested, $code) = @_;
    return { requested => $requested, resolved => JSON::PP::false } unless $code;
    my $cv = B::svref_2object($code);
    my $glob = $cv->GV;
    my $body = eval { B::Deparse->new('-p', '-sC')->coderef2text($code) };
    return {
        requested => $requested, resolved => JSON::PP::true,
        actual_name => eval { $glob->STASH->NAME . '::' . $glob->NAME },
        prototype => prototype($code),
        implementation => (eval { $cv->XSUB } ? 'xsub' : 'perl'),
        file => eval { $cv->FILE },
        body_sha256 => (defined $body ? sha256_hex($body) : undef),
        provider_file => _file(eval { $cv->FILE }),
    };
}

sub _scalar {
    my ($value) = @_;
    return { domain => 'undefined', supported => JSON::PP::false } unless defined $value;
    return { domain => 'reference', reference => ref($value), supported => JSON::PP::false }
        if ref $value;
    my $flag = utf8::is_utf8($value) ? JSON::PP::true : JSON::PP::false;
    my $bytes = $flag ? pack('C0U*', unpack('U*', $value)) : $value;
    return { domain => 'scalar', supported => JSON::PP::true, utf8_flag => $flag,
        bytes_hex => unpack('H*', $bytes), char_length => length($value) };
}

sub _call {
    my ($function) = @_;
    my ($value, $error, $warning);
    {
        local $SIG{'__WARN__'} = sub { $warning .= shift };
        my $ok = eval { $value = $function->(); 1 };
        $error = $@ unless $ok;
    }
    return { result => _scalar($value), error => $error || undef, warning => $warning || undef };
}

sub _vectors {
    my @boundaries = map {
        my ($name, $point) = @$_;
        [$name, sub { my $value = chr $point; utf8::upgrade($value); $value }]
    } (['u007f', 0x7f], ['u0080', 0x80], ['u07ff', 0x7ff], ['u0800', 0x800],
       ['uffff', 0xffff], ['u10000', 0x10000], ['u10ffff', 0x10ffff]);
    my $reference = 'r';
    my @cases = (
        ['forced_utf8_ascii', sub { my $value = 'a'; utf8::upgrade($value); $value }],
        ['forced_utf8_latin1', sub { my $value = "\xe9"; utf8::upgrade($value); $value }],
        @boundaries,
        ['unicode_nul', sub { "é\0" }],
        ['bytes_utf8', sub { pack('H*', 'c3a9') }],
        ['bytes_nul', sub { pack('H*', '610062') }],
        ['undefined', sub { undef }], ['scalar_reference', sub { \$reference }],
        ['surrogate', sub { chr 0xd800 }], ['out_of_range', sub { chr 0x110000 }],
    );
    my @observed;
    for my $case (@cases) {
        my ($name, $factory) = @$case;
        my ($value, $creation_error);
        {
            local $SIG{'__WARN__'} = sub {};
            my $ok = eval { $value = $factory->(); 1 };
            $creation_error = $@ unless $ok;
        }
        push @observed, {
            name => $name, creation_error => $creation_error || undef, input => _scalar($value),
            is_utf8 => _call(sub { Encode::is_utf8($value) }),
            encode_utf8 => _call(sub { Encode::encode('utf8', $value) }),
        };
    }
    return \@observed;
}

sub snapshot_in_process {
    return { ok => JSON::PP::false, error => 'Encode not loaded; refusing to load final provider' }
        unless exists $INC{'Encode.pm'};
    my $encoding = Encode::find_encoding('utf8');
    my $method = $encoding ? $encoding->can('encode') : undef;
    my %loaded = map { $_ => _file($INC{$_}) } sort grep { /\AEncode(?:\.pm|\/)/ } keys %INC;
    my @shared = map { _file($_) } grep { /(?:^|\/)Encode(?:\/|\.)/ } @DynaLoader::dl_shared_objects;
    return {
        ok => JSON::PP::true, instrument => 'oxidex_utf8_primitive_contract_v1',
        perl => { executable => $^X, executable_sha256 => _file($^X)->{sha256},
            version => $], archname => $Config::Config{archname} },
        encode_pm => _file($INC{'Encode.pm'}), encode_inc => \%loaded, encode_shared_objects => \@shared,
        bindings => { encode => _cv('Encode::encode', \&Encode::encode),
            is_utf8 => _cv('Encode::is_utf8', \&Encode::is_utf8) },
        utf8_registry => { class => ref($encoding), name => eval { $encoding->name },
            encode_method => _cv('utf8 registry encode', $method) },
        vectors => _vectors(),
        unresolved_source_links => 'core CV-to-binary and source-to-binary linkage intentionally not asserted',
        admission => JSON::PP::false,
    };
}

sub portable_snapshot {
    my ($raw) = @_;
    return { kind => 'utf8_primitive_snapshot_v1', ok => JSON::PP::false,
        error => 'native_snapshot_unavailable' }
        unless ref($raw) eq 'HASH' && $raw->{ok};
    my $portable_cv = sub {
        my ($cv) = @_;
        return { resolved => JSON::PP::false } unless ref($cv) eq 'HASH';
        return { map { $_ => $cv->{$_} }
            qw(requested resolved actual_name implementation prototype body_sha256) };
    };
    # Raw unsupported reference results can contain process addresses. Keep
    # these diagnostics outside deterministic artifacts; their domain remains
    # an explicit capability gap, not an omitted passing test.
    my %unsupported = map { $_ => 1 } qw(scalar_reference surrogate out_of_range);
    return {
        kind => 'utf8_primitive_snapshot_v1', ok => JSON::PP::true,
        perl_version => $raw->{perl}{version},
        bindings => { map { $_ => $portable_cv->($raw->{bindings}{$_}) } qw(encode is_utf8) },
        utf8_registry => {
            class => $raw->{utf8_registry}{class}, name => $raw->{utf8_registry}{name},
            encode_method => $portable_cv->($raw->{utf8_registry}{encode_method}),
        },
        vectors => [ grep { !$unsupported{$_->{name}} } @{$raw->{vectors}} ],
        unsupported_domains => [ sort keys %unsupported ],
    };
}

sub _save_diagnostics {
    my ($label, $raw) = @_;
    my $directory = $ENV{OXIDEX_UTF8_EVIDENCE_DIR};
    return unless defined $directory;
    die 'OXIDEX_UTF8_EVIDENCE_DIR must be an existing directory' unless -d $directory;
    my ($fh, $path) = tempfile("utf8-${label}-XXXXXX", DIR => $directory, UNLINK => 0);
    binmode $fh;
    print $fh JSON::PP->new->canonical->utf8->encode($raw);
    close $fh or die 'cannot save UTF8 primitive diagnostics';
}

sub capture_pristine {
    my ($perl, $timeout) = @_;
    $timeout //= 30;
    my ($out, $out_path) = tempfile(UNLINK => 1);
    my ($err, $err_path) = tempfile(UNLINK => 1);
    my $module_root = dirname(dirname(abs_path(__FILE__)));
    my $program = q{
        die 'Encode unexpectedly preloaded' if exists $INC{'Encode.pm'};
        require Encode;
        print JSON::PP->new->canonical->utf8->encode(
            OxiDex::Utf8PrimitiveContract::snapshot_in_process());
    };
    my $pid = fork();
    return { ok => JSON::PP::false, error => 'pristine_fork_failed' } unless defined $pid;
    if (!$pid) {
        delete @ENV{grep { /^PERL5/ || $_ eq 'PERLLIB' } keys %ENV};
        open STDOUT, '>&', $out or POSIX::_exit(127);
        open STDERR, '>&', $err or POSIX::_exit(127);
        exec $perl, '-I', $module_root, '-MJSON::PP', '-MOxiDex::Utf8PrimitiveContract', '-e', $program;
        POSIX::_exit(127);
    }
    my $deadline = time() + $timeout;
    my $status;
    while (1) {
        if (waitpid($pid, WNOHANG) == $pid) { $status = $?; last; }
        if (time() >= $deadline) {
            kill 'TERM', $pid;
            sleep 0.05;
            kill 'KILL', $pid if waitpid($pid, WNOHANG) == 0;
            waitpid($pid, 0);
            return { ok => JSON::PP::false, error => 'pristine_timeout' };
        }
        sleep 0.01;
    }
    seek($out, 0, 0); seek($err, 0, 0);
    local $/;
    my $output = <$out>; my $diagnostic = <$err>;
    my $raw = eval { JSON::PP::decode_json($output // '') };
    _save_diagnostics('pristine', { exit => $status, stdout => $output, stderr => $diagnostic });
    return { ok => JSON::PP::false, error => 'pristine_failed' }
        unless $status == 0 && ref($raw) eq 'HASH';
    return portable_snapshot($raw);
}

sub capture_final {
    # Execute in the actual table producer after its complete module load.
    # A missing/overridden provider cannot be repaired by loading another one.
    my $raw = eval { snapshot_in_process() };
    $raw = { ok => JSON::PP::false, error => 'final_snapshot_failed' }
        unless ref($raw) eq 'HASH';
    _save_diagnostics('final', $raw);
    return portable_snapshot($raw);
}

1;
