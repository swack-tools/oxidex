# Runner image for the IONOS ARC scale sets (ionos-large / ionos-small).
#
# Everything oxidex CI installs at job start on ubuntu-latest is baked in
# here, so jobs don't spend their first minutes in apt. The base image is
# Ubuntu 24.04 (noble), the same release as GitHub's ubuntu-latest today;
# system perl is 5.38.2, which matches the pinned oracle's perl.
#
# Build (Apple Silicon needs the platform flag):
#   docker buildx build --platform linux/amd64 \
#     -t ghcr.io/swack-tools/ci-runner:<tag> --push .github/ionos-runners/image
ARG RUNNER_VERSION=2.337.0
FROM ghcr.io/actions/actions-runner:${RUNNER_VERSION}

USER root
ENV DEBIAN_FRONTEND=noninteractive

# oxidex build + CI tool dependencies. libarchive-zip-perl is ExifTool's
# Archive::Zip (without it .docx reads as ZIP); ci.yml's cpanm step then
# finds it already installed.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config libssl-dev cmake clang \
      perl cpanminus libarchive-zip-perl \
      python3 python3-venv python3-pip \
      git git-lfs curl wget ca-certificates gnupg \
      unzip zip xz-utils zstd jq file rsync time bc tzdata locales \
 && install -d -m 0755 /etc/apt/keyrings \
 && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
      -o /etc/apt/keyrings/githubcli-archive-keyring.gpg \
 && echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
      > /etc/apt/sources.list.d/github-cli.list \
 && apt-get update && apt-get install -y --no-install-recommends gh \
 && sed -i 's/^# *en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen && locale-gen \
 && rm -rf /var/lib/apt/lists/*

ENV LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

# The pinned toolchain from rust-toolchain.toml, preinstalled for the runner
# user; dtolnay/rust-toolchain finds rustup and only verifies the pin.
USER runner
ENV CARGO_HOME=/home/runner/.cargo RUSTUP_HOME=/home/runner/.rustup
ENV PATH=/home/runner/.cargo/bin:${PATH}
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
      --default-toolchain 1.97.1 --component clippy,rustfmt \
 && rustc -vV && cargo -V \
 && perl -MArchive::Zip -e 'print "Archive::Zip $Archive::Zip::VERSION\n"' \
 && python3 --version && ldd --version | head -1
