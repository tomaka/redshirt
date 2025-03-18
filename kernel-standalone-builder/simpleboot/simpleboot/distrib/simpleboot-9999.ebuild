# Copyright (C) 2023 dzsolt
# Distributed under the terms of the GNU General Public License v2

EAPI=7

DESCRIPTION="Dependency-free, all-in-one boot loader and bootable disk image creator."
HOMEPAGE="https://gitlab.com/bztsrc/simpleboot"

LICENSE="MIT"
SLOT="0"
IUSE="rebuild"

# If PV starts with 9999, use git-r3 for version control
if [[ ${PV} == 9999* ]]; then
    inherit git-r3
    EGIT_REPO_URI='https://gitlab.com/bztsrc/simpleboot.git'
else
    SRC_URI="https://gitlab.com/bztsrc/simpleboot/-/archive/${PV}/simpleboot-${PV}.tar.gz -> ${P}.tar.gz"
    KEYWORDS="~amd64 ~x86 ~arm64 ~arm"
fi

BDEPEND="
    rebuild? (
        dev-lang/fasm
        sys-devel/llvm
        sys-devel/lld
    )
"

src_prepare() {
    default
    # Nothing specific to prepare
}

src_compile() {
    if use rebuild; then
        emake -C src distclean || die "Failed to execute 'make -C src distclean'"
    fi
    emake -C src -j1 || die "Failed to build simpleboot"
}

src_install() {
    dobin src/simpleboot || die "Failed to install simpleboot"
    doman src/misc/simpleboot.1.gz
    doheader simpleboot.h
}
