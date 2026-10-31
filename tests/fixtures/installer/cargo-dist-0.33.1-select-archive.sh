# select_archive_for_arch() and check_glibc() from the cargo-dist 0.33.1 shell
# installer generated for ts-crap v2.2.1 (lines 554-681), unmodified.
select_archive_for_arch() {
    local _true_arch="$1"
    local _archive

    # try each archive, checking runtime conditions like libc versions
    # accepting the first one that matches, as it's the best match
    case "$_true_arch" in 
        "aarch64-apple-darwin")
            _archive="ts-crap-aarch64-apple-darwin.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            _archive="ts-crap-x86_64-apple-darwin.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "aarch64-pc-windows-msvc")
            _archive="ts-crap-x86_64-pc-windows-msvc.zip"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "aarch64-unknown-linux-gnu")
            _archive="ts-crap-aarch64-unknown-linux-gnu.tar.xz"
            if ! check_glibc "2" "43"; then
                _archive=""
            fi
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            _archive="ts-crap-aarch64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "aarch64-unknown-linux-musl-dynamic")
            _archive="ts-crap-aarch64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "aarch64-unknown-linux-musl-static")
            _archive="ts-crap-aarch64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-apple-darwin")
            _archive="ts-crap-x86_64-apple-darwin.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-pc-windows-gnu")
            _archive="ts-crap-x86_64-pc-windows-msvc.zip"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-pc-windows-msvc")
            _archive="ts-crap-x86_64-pc-windows-msvc.zip"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-unknown-linux-gnu")
            _archive="ts-crap-x86_64-unknown-linux-gnu.tar.xz"
            if ! check_glibc "2" "43"; then
                _archive=""
            fi
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            _archive="ts-crap-x86_64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-unknown-linux-musl-dynamic")
            _archive="ts-crap-x86_64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        "x86_64-unknown-linux-musl-static")
            _archive="ts-crap-x86_64-unknown-linux-musl.tar.xz"
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            ;;
        *)
            err "there isn't a download for your platform $_true_arch"
            ;;
    esac
    err "no compatible downloads were found for your platform $_true_arch"
}

check_glibc() {
    local _min_glibc_major="$1"
    local _min_glibc_series="$2"

    # Parsing version out from line 1 like:
    # ldd (Ubuntu GLIBC 2.35-0ubuntu3.1) 2.35
    _local_glibc="$(ldd --version | awk -F' ' '{ if (FNR<=1) print $NF }')"

    if [ "$(echo "${_local_glibc}" | awk -F. '{ print $1 }')" = "$_min_glibc_major" ] && [ "$(echo "${_local_glibc}" | awk -F. '{ print $2 }')" -ge "$_min_glibc_series" ]; then
        return 0
    else
        say "System glibc version (\`${_local_glibc}') is too old; checking alternatives" >&2
        return 1
    fi
}

