#!/usr/bin/env bash

nexshell_clear_nested_cargo_env() {
    local name

    for name in \
        CARGO_BIN_NAME \
        CARGO_CRATE_NAME \
        CARGO_MANIFEST_DIR \
        CARGO_MANIFEST_LINKS \
        CARGO_PKG_AUTHORS \
        CARGO_PKG_DESCRIPTION \
        CARGO_PKG_HOMEPAGE \
        CARGO_PKG_LICENSE \
        CARGO_PKG_LICENSE_FILE \
        CARGO_PKG_NAME \
        CARGO_PKG_README \
        CARGO_PKG_REPOSITORY \
        CARGO_PKG_RUST_VERSION \
        CARGO_PKG_VERSION \
        CARGO_PKG_VERSION_MAJOR \
        CARGO_PKG_VERSION_MINOR \
        CARGO_PKG_VERSION_PATCH \
        CARGO_PKG_VERSION_PRE \
        CARGO_PRIMARY_PACKAGE \
        CARGO_TARGET_TMPDIR \
        DEBUG \
        HOST \
        NUM_JOBS \
        OPT_LEVEL \
        OUT_DIR \
        PROFILE \
        TARGET; do
        unset "$name"
    done

    for name in ${!CARGO_CFG_@} ${!CARGO_FEATURE_@} ${!DEP_@}; do
        unset "$name"
    done
}

nexshell_run_cargo_clean() {
    (
        nexshell_clear_nested_cargo_env
        cargo "$@"
    )
}
