# Originally generated by rust2rpm 16
%define dracutlibdir %{_prefix}/lib/dracut
%bcond_without check
%global __cargo_skip_build 0
# The library is for internal code reuse and is not a public API
%global __cargo_is_lib 0

# This commit will be the one that marks the code that will be
# available as an PRM, source (on main): 7flying/coreos/coreos-installer-dracut
%global dracutcommit 95a46c66fc9d0c4129097d227bc7e199274a8088
%global dracutshortcommit %(c=%{dracutcommit}; echo ${c:0:7})

%global crate coreos-installer

Name:           rust-%{crate}
Version:        0.15.0
Release:        5%{?dist}
Summary:        Installer for Fedora CoreOS and RHEL CoreOS

# Upstream license specification: Apache-2.0
License:        ASL 2.0
URL:            https://crates.io/crates/coreos-installer
Source0:        https://crates.io/api/v1/crates/%{crate}/%{version}/download#/%{crate}-%{version}.crate
# not used on Fedora
Source1:        https://github.com/coreos/%{crate}/releases/download/v%{version}/%{crate}-%{version}-vendor.tar.gz
Source2:        https://github.com/7flying/coreos-installer-dracut/archive/%{dracutcommit}/coreos-installer-dracut-%{dracutshortcommit}.tar.gz
#Source2:        https://github.com/coreos/coreos-installer-dracut/archive/%{dracutcommit}/coreos-installer-dracut-%{dracutshortcommit}.tar.gz

# The RHEL 8 rust-toolset macros don't let us enable features from the
# %%cargo_* macros.  Enable rdcore directly in Cargo.toml.
Patch0:         enable-rdcore.patch

ExclusiveArch:  %{rust_arches}
%if 0%{?rhel} && !0%{?eln}
BuildRequires:  rust-toolset
BuildRequires:  openssl-devel
# To ensure we're not bundling system libraries
BuildRequires:  xz-devel
%else
BuildRequires:  rust-packaging
%endif
BuildRequires:  systemd-rpm-macros
# For tests
BuildRequires:  gnupg2

%global _description %{expand:
coreos-installer installs Fedora CoreOS or RHEL CoreOS to bare-metal
machines (or, occasionally, to virtual machines).
}

%description %{_description}

%package     -n %{crate}
Summary:        %{summary}
# ASL 2.0
# ASL 2.0 or Boost
# MIT
# MIT or ASL 2.0
# Unlicense or MIT
# zlib
License:        ASL 2.0 and MIT and zlib

Requires:       gnupg
Requires:       kpartx
Requires:       systemd-udev
Requires:       util-linux
%ifarch s390x
# This should be spelled "s390utils-core" but some of the binaries are
# still moving over from s390utils-base
Requires:       /usr/sbin/chreipl
Requires:       /usr/sbin/dasdfmt
Requires:       /usr/sbin/fdasd
Requires:       /usr/sbin/lszdev
Requires:       /usr/sbin/zipl
%endif

# Since `rust-coreos-installer` creates a `coreos-installer`
# subpackage with a newer version number, which supersedes the
# deprecated `coreos-installer` package (https://src.fedoraproject.org/rpms/coreos-installer),
# an explicit `Obsoletes:` for `coreos-installer` is not necessary.

# Obsolete dracut modules as they are not provided in this package.
Obsoletes:      coreos-installer-dracut < 0.0.1

%description -n %{crate} %{_description}

%prep
%autosetup -n %{crate}-%{version} -p1 -a 2
%if 0%{?rhel} && !0%{?eln}
tar xvf %{SOURCE1}
mkdir -p .cargo
cat >.cargo/config << EOF
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF
%else
%cargo_prep
%endif
# Fix SIGSEGV in tests on s390x
# https://bugzilla.redhat.com/show_bug.cgi?id=1883457
sed -i 's/"-Ccodegen-units=1",//' .cargo/config

%if !0%{?rhel} || 0%{?eln}
%generate_buildrequires
%cargo_generate_buildrequires -f rdcore
%endif

%build
%if 0%{?rhel} && !0%{?eln}
%cargo_build
%else
%cargo_build -f rdcore
%endif

%install
%if 0%{?rhel} && !0%{?eln}
%make_install RELEASE=1
# 51coreos-installer for coreos-installer-dracut
%make_install -C coreos-installer-dracut-%{dracutcommit}
%else
%cargo_install -f rdcore
# Install binaries, dracut modules, units, targets, generators for running via systemd
install -D -m 0755 -t %{buildroot}%{dracutlibdir}/modules.d/50rdcore dracut/50rdcore/module-setup.sh
make install-scripts DESTDIR=%{buildroot}
make install-systemd DESTDIR=%{buildroot}
make install-man DESTDIR=%{buildroot}
mv %{buildroot}%{_bindir}/rdcore %{buildroot}%{dracutlibdir}/modules.d/50rdcore/
%endif

%package     -n %{crate}-bootinfra
Summary:     %{crate} boot-time infrastructure for use on Fedora/RHEL CoreOS
Requires:    %{crate}%{?_isa} = %{?epoch:%{epoch}:}%{version}-%{release}
# ASL 2.0
# ASL 2.0 or Boost
# MIT
# MIT or ASL 2.0
# Unlicense or MIT
# zlib
License:     ASL 2.0 and MIT and zlib

# Package was renamed from coreos-installer-systemd when rdcore was added
Provides:    %{crate}-systemd = %{version}-%{release}
Obsoletes:   %{crate}-systemd <= 0.3.0-3

%description -n %{crate}-bootinfra
This subpackage contains boot-time infrastructure for Fedora CoreOS and
RHEL CoreOS.  It is not needed on other platforms.

%files       -n %{crate}-bootinfra
%{dracutlibdir}/modules.d/50rdcore
%{_libexecdir}/*
%{_unitdir}/*
%{_systemdgeneratordir}/*

%if 0%{?rhel} && !0%{?eln}
%package     -n %{crate}-dracut
Summary:     Dracut module for running coreos-installer in the initrd
Requires:    %{crate} = %{version}-%{release}

%description -n %{crate}-dracut
This subpackage contains files and configuration to run coreos-installer
from the initramfs.

%files       -n %{crate}-dracut
%{dracutlibdir}/modules.d/51coreos-installer
%endif

%files       -n %{crate}
%license LICENSE
%doc README.md
%{_bindir}/coreos-installer
%{_mandir}/man8/*

%if %{with check}
%check
%if 0%{?rhel} && !0%{?eln}
%cargo_test
%else
%cargo_test -f rdcore
%endif
%endif

%changelog
* Tue Jul 12 2022 Michael Armijo <marmijo@redhat.com> - 0.15.0-1
- New release
- Sync with Fedora package
- Install coreos-installer-dracut with "make install"

* Thu Feb 24 2022 Antonio Murdaca <runcom@linux.com> - 0.11.0-4
- update coi-dracut to fix growfs service Before

* Thu Feb 10 2022 Antonio Murdaca <runcom@linux.com> - 0.11.0-3
- update coi-dracut to support default poweroff behavior

* Wed Jan 26 2022 Antonio Murdaca <runcom@linux.com> - 0.11.0-2
- Bump coreos-installer-dracut to support encrypted rootfs

* Wed Dec 15 2021 Sohan Kunkerkar <skunkerk@redhat.com> - 0.11.0-1
- New release
- Fix inadvertent inclusion of coreos-installer-dracut in -bootinfra on RHEL
- Have -dracut own 51coreos-installer directory on RHEL

* Tue Nov 30 2021 Sohan Kunkerkar <skunkerk@redhat.com> - 0.10.1-6
- Vendor rust dependencies on RHEL only
- Add coreos-installer-dracut subpackage on RHEL only

* Fri Nov 26 2021 Antonio Murdaca <runcom@linux.com> - 0.10.1-5
- fix https://bugzilla.redhat.com/show_bug.cgi?id=2027255

* Thu Nov 18 2021 Antonio Murdaca <runcom@linux.com> - 0.10.1-4
- fix dracut module dependencies

* Wed Nov 17 2021 Antonio Murdaca <runcom@linux.com> - 0.10.1-3
- fix dracut module installation target

* Tue Oct 26 2021 Antonio Murdaca <runcom@linux.com> - 0.10.1-2
- rebuilt

* Mon Oct 25 2021 Antonio Murdaca <runcom@linux.com> - 0.10.1-1
- New release

* Thu Sep 16 11:20:52 CET 2021 Antonio Murdaca <amurdaca@redhat.com> - 0.10.0-1
- New bumped release

* Mon Dec 28 13:28:50 CET 2020 Igor Raits <ignatenkobrain@fedoraproject.org> - 0.7.2-2
- Rebuild

* Thu Oct 22 2020 Sohan Kunkerkar <skunkerk@redhat.com> - 0.7.2-1
- New release

* Tue Oct 06 2020 Dusty Mabe <dusty@dustymabe.com> - 0.7.0-4
- Backport commit to start coreos-installer service after systemd-resolved
    - https://github.com/coreos/coreos-installer/pull/389

* Thu Oct 01 2020 Dusty Mabe <dusty@dustymabe.com> - 0.7.0-3
- Backport commit to add F33 and F34 keys. Drop F31 keys.
    - https://github.com/coreos/coreos-installer/pull/387

* Wed Sep 30 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.7.0-2
- Fix SIGSEGV in tests on s390x

* Mon Sep 21 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.7.0-1
- New release

* Tue Aug 25 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.6.0-1
- New release

* Sun Aug 16 15:01:11 GMT 2020 Igor Raits <ignatenkobrain@fedoraproject.org> - 0.5.0-2
- Rebuild

* Fri Jul 31 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.5.0-1
- New release

* Wed Jul 29 2020 Fedora Release Engineering <releng@fedoraproject.org> - 0.4.0-2
- Rebuilt for https://fedoraproject.org/wiki/Fedora_33_Mass_Rebuild

* Fri Jul 24 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.4.0-1
- New release
- Rename -systemd subpackage to -bootinfra
- Add rdcore Dracut module to -bootinfra

* Fri Jul 24 2020 Igor Raits <ignatenkobrain@fedoraproject.org> - 0.3.0-2
- Rebuild

* Mon Jul 13 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.3.0-1
- New release

* Sat May 30 2020 Igor Raits <ignatenkobrain@fedoraproject.org> - 0.2.1-2
- Fixup license

* Fri May 29 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.2.1-1
- New release
- Make coreos-installer-{service,generator} world-readable

* Tue May 05 2020 Robert Fairley <rfairley@redhat.com> - 0.2.0-1
- Update to 0.2.0

* Sat Mar 21 2020 Benjamin Gilbert <bgilbert@redhat.com> - 0.1.3-1
- New release

* Fri Feb 21 2020 Josh Stone <jistone@redhat.com> - 0.1.2-4
- Bump to nix 0.17 and reqwest 0.10

* Thu Jan 30 2020 Fedora Release Engineering <releng@fedoraproject.org> - 0.1.2-3
- Rebuilt for https://fedoraproject.org/wiki/Fedora_32_Mass_Rebuild

* Thu Jan 09 2020 Josh Stone <jistone@redhat.com> - 0.1.2-2
- Remove the nix downgrade.

* Wed Jan 08 2020 Dusty Mabe <dusty@dustymabe.com> - 0.1.2-1
- Bump to new upstream release 0.1.2
    - Release notes: https://github.com/coreos/coreos-installer/releases/tag/v0.1.2
- Update spec file to include systemd units from upstream
    - These were added upstream in https://github.com/coreos/coreos-installer/pull/119

* Fri Dec 20 17:57:28 UTC 2019 Robert Fairley <rfairley@redhat.com> - 0.1.1-1
- Initial package
