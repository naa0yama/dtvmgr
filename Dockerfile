# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769
#- -------------------------------------------------------------------------------------------------
#- Global
#-
ARG DEBIAN_FRONTEND=noninteractive \
	TZ=${TZ:-Asia/Tokyo} \
	USER_NAME=cuser \
	USER_UID=${USER_UID:-60001} \
	USER_GID=${USER_GID:-${USER_UID}}

## renovate: datasource=github-releases packageName=rui314/mold versioning=semver automerge=true
ARG MOLD_VERSION=v2.41.0

# Rust tools
## renovate: datasource=github-releases packageName=mozilla/sccache versioning=semver automerge=true
ARG SCCACHE_VERSION=v0.14.0

# retry dns and some http codes that might be transient errors
ARG CURL_OPTS="-sfSL --retry 3 --retry-delay 2 --retry-connrefused"


#- -------------------------------------------------------------------------------------------------
#- Builder Base
#-
FROM --platform=$BUILDPLATFORM rust:1.94.1-trixie@sha256:652612f07bfbbdfa3af34761c1e435094c00dde4a98036132fca28c7bb2b165c AS builder-base
ARG CURL_OPTS \
	DEBIAN_FRONTEND \
	MOLD_VERSION \
	SCCACHE_VERSION \
	USER_NAME \
	USER_UID \
	USER_GID \
	TZ

ENV LANG=C.UTF-8 LC_ALL=C.UTF-8

SHELL [ "/bin/bash", "-c" ]

RUN echo "**** set Timezone ****" && \
	set -euxo pipefail && \
	ln -snf /usr/share/zoneinfo/${TZ} /etc/localtime && echo ${TZ} > /etc/timezone

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
	--mount=type=cache,target=/var/lib/apt,sharing=locked \
	\
	echo "**** Dependencies ****" && \
	rm -f /etc/apt/apt.conf.d/docker-clean && \
	echo 'Binary::apt::APT::Keep-Downloaded-Packages "true";' > /etc/apt/apt.conf.d/keep-cache && \
	echo "**** Dependencies ****" && \
	set -euxo pipefail && \
	apt-get -y update && \
	apt-get -y upgrade && \
	apt-get -y install --no-install-recommends \
	bash \
	bash-completion \
	ca-certificates \
	curl \
	git \
	gnupg \
	jq \
	musl-tools \
	nano \
	sudo \
	wget

# gh-sync:keep-start
# Project-specific dependencies are listed here.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
	--mount=type=cache,target=/var/lib/apt,sharing=locked \
	\
	echo "**** Dependencies | project ****" && \
	set -euxo pipefail && \
	apt-get -y install --no-install-recommends \
	sqlite3

# gh-sync:keep-end

RUN echo "**** Create user ****" && \
	set -euxo pipefail && \
	groupadd --gid "${USER_GID}" "${USER_NAME}" && \
	useradd -s /bin/bash --uid "${USER_UID}" --gid "${USER_GID}" -m "${USER_NAME}" && \
	echo "${USER_NAME}:password" | chpasswd && \
	passwd -d "${USER_NAME}"

RUN echo "**** Add sudo user ****" && \
	set -euxo pipefail && \
	echo -e "${USER_NAME}\tALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/${USER_NAME}"

RUN echo "**** Install mold ****" && \
	set -euxo pipefail && \
	_release_data="$(curl ${CURL_OPTS} -H 'User-Agent: builder/1.0' \
	https://api.github.com/repos/rui314/mold/releases/tags/${MOLD_VERSION})" && \
	_asset="$(echo "$_release_data" | jq -r '.assets[] | select(.name | endswith("-x86_64-linux.tar.gz"))')" && \
	_download_url="$(echo "$_asset" | jq -r '.browser_download_url')" && \
	_digest="$(echo "$_asset" | jq -r '.digest')" && \
	_sha256="${_digest#sha256:}" && \
	_filename="$(basename "$_download_url")" && \
	curl ${CURL_OPTS} -H 'User-Agent: builder/1.0' -o "./${_filename}" "${_download_url}" && \
	echo "${_sha256}  ${_filename}" | sha256sum -c - && \
	tar -xvf "./${_filename}" --strip-components 1 -C /usr && \
	type -p mold && \
	rm -rf "./${_filename}"

RUN echo "**** Rust tool sccache ****" && \
	set -euxo pipefail && \
	_download_url="$(curl ${CURL_OPTS} -H 'User-Agent: builder/1.0' \
	https://api.github.com/repos/mozilla/sccache/releases/tags/${SCCACHE_VERSION} | \
	jq -r '.assets[] | select(.name | startswith("sccache-v") and endswith("-x86_64-unknown-linux-musl.tar.gz")) | .browser_download_url')" && \
	_filename="$(basename "$_download_url")" && \
	_tmpdir=$(mktemp -q -d) && \
	curl ${CURL_OPTS} -H 'User-Agent: builder/1.0' -o "./${_filename}" "${_download_url}" && \
	tar -xvf "./${_filename}" --strip-components 1 -C "${_tmpdir}" && \
	ls -lah "${_tmpdir}" && \
	cp -av "${_tmpdir}/sccache" /usr/local/bin/ && \
	type -p sccache && \
	rm -rf "./${_filename}" "${_tmpdir}"

RUN --mount=type=bind,source=rust-toolchain.toml,target=/rust-toolchain.toml \
	\
	echo "**** Rust component ****" && \
	set -euxo pipefail && \
	cargo -V


# gh-sync:keep-start
#- -------------------------------------------------------------------------------------------------
#- Copy libs
#-
FROM ghcr.io/naa0yama/join_logo_scp_trial:v26.03.08.01-ubuntu2404@sha256:faaf043461e7f8d05a3cbdd7d5e20756418e99b06ed60061853827e7489dc725 AS jlse
# gh-sync:keep-end
#- -------------------------------------------------------------------------------------------------
#- Development
#-
FROM --platform=$BUILDPLATFORM builder-base AS development
ARG CURL_OPTS \
	DEBIAN_FRONTEND \
	USER_NAME

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
	--mount=type=cache,target=/var/lib/apt,sharing=locked \
	\
	echo "**** Dependencies ****" && \
	set -euxo pipefail && \
	apt-get -y install --no-install-recommends \
	shellcheck

# User level settings
USER ${USER_NAME}
ENV CARGO_HOME=/home/${USER_NAME}/.cargo

RUN echo "**** Directory Create ****" && \
	set -euxo pipefail && \
	mkdir -p \
	~/.config \
	~/.config/mise \
	~/.local \
	~/.local/bin \
	~/.local/share \
	~/.local/share/claude \
	~/.local/share/mise

RUN echo "**** Create ${CARGO_HOME} ****" && \
	set -euxo pipefail && \
	mkdir -p "${CARGO_HOME}"

RUN printf '%s\n' \
	'case ":$PATH:" in' \
	'  *:"$CARGO_HOME/bin":*) ;;' \
	'  *) export PATH="$CARGO_HOME/bin:$PATH" ;;' \
	'esac' >> ~/.bashrc

RUN echo "**** Rust bash-completion ****" && \
	set -euxo pipefail && \
	mkdir -p                         /home/${USER_NAME}/.local/share/bash-completion/completions && \
	rustup completions bash cargo  > /home/${USER_NAME}/.local/share/bash-completion/completions/cargo && \
	rustup completions bash rustup > /home/${USER_NAME}/.local/share/bash-completion/completions/rustup

RUN <<EOF
echo "**** add '~/.bashrc mise and claude code ****"
set -euxo pipefail

cat <<- '_DOC_' >> ~/.bashrc
# mise
eval "$(~/.local/bin/mise activate bash)"

# This requires bash-completion to be installed
if [ ! -f "${HOME}/.local/share/bash-completion/completions/mise" ]; then
	~/.local/bin/mise use -g usage
	mkdir -p "${HOME}/.local/share/bash-completion/completions/"
	~/.local/bin/mise completion bash --include-bash-completion-lib > "${HOME}/.local/share/bash-completion/completions/mise"
fi

# ~/.local/bin (Claude Code, OpenObserve, etc.)
case ":$PATH:" in
	*:"$HOME/.local/bin":*) ;;
	*) export PATH="$HOME/.local/bin:$PATH" ;;
esac
alias cc="claude --dangerously-skip-permissions"

_DOC_
EOF

# gh-sync:keep-start
# Project-specific dependencies are listed here.
USER root

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
	--mount=type=cache,target=/var/lib/apt,sharing=locked \
	\
	echo "**** Dependencies | FFmpeg runtime ****" && \
	rm -f /etc/apt/apt.conf.d/docker-clean && \
	echo 'Binary::apt::APT::Keep-Downloaded-Packages "true";' > /etc/apt/apt.conf.d/keep-cache && \
	echo "**** Dependencies ****" && \
	set -euxo pipefail && \
	apt-get -y update && \
	apt-get -y upgrade && \
	apt-get -y install --no-install-recommends \
	# DRM and mp3lame
	libdrm2 libmp3lame0 \
	# FFmpeg filter drawtext
	libharfbuzz0b libfribidi0

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
	--mount=type=cache,target=/var/lib/apt,sharing=locked \
	\
	echo "**** Dependencies | tsduck ****" && \
	set -euxo pipefail && \
	_tsduckdownload_url="$(curl ${CURL_OPTS} -H "User-Agent: builder/1.0" -sfSL https://api.github.com/repos/tsduck/tsduck/releases/latest | \
	jq -r '.assets[] | select(.name | startswith("tsduck_") and endswith("debian13_amd64.deb")) | .browser_download_url')" && \
	_tsduck_filename="$(basename "$_tsduckdownload_url")" && \
	curl ${CURL_OPTS} -o "./${_tsduck_filename}" "${_tsduckdownload_url}" && \
	ls -lah && \
	apt-get -y install "./${_tsduck_filename}" && \
	tsp --version && \
	rm -rf "./${_tsduck_filename}"

# User level settings
USER ${USER_NAME}

RUN printf '%s\n' \
	'# dtvmgr' \
	'eval "$(dtvmgr completion bash)"' >> ~/.bashrc

ENV PKG_CONFIG_PATH=/opt/ffmpeg/lib/pkgconfig \
	LD_LIBRARY_PATH=/opt/ffmpeg/lib \
	LIBVA_DRIVERS_PATH=/opt/ffmpeg/lib/dri \
	PATH="/opt/ffmpeg/bin:${PATH}"

COPY --from=jlse 			/join_logo_scp_trial							/join_logo_scp_trial
COPY --from=jlse 			/opt/ffmpeg										/opt/ffmpeg

RUN echo  "**** FFmpeg | check library ****" && \
	set -euxo pipefail && \
	find /usr/lib/x86_64-linux-gnu -name 'libdrm.so*' -or -name 'libmp3lame.so*' && \
	/opt/ffmpeg/bin/ffmpeg -hide_banner -hwaccels && \
	/opt/ffmpeg/bin/ffmpeg -hide_banner -buildconf && \
	for i in decoders encoders; do echo ${i}:; /opt/ffmpeg/bin/ffmpeg -hide_banner -${i} | \
	egrep -i "[x|h]264|[x|h]265|av1|cuvid|hevc|libmfx|nv[dec|enc]|qsv|vaapi|vp9"; done

# gh-sync:keep-end
