.PHONY: srpm
srpm:
	dnf install --nogpgcheck -y git rust-packaging rpm-build rpmdevtools
	# similar to https://github.com/actions/checkout/issues/760, but for COPR
	git config --global --add safe.directory '*'
	# curl -LOf https://src.fedoraproject.org/rpms/rust-coreos-installer/raw/rawhide/f/rust-coreos-installer.spec
	curl -LOf https://raw.githubusercontent.com/7flying/coreos-installer/main/rust-coreos-installer-gitlab.spec
	version=$$(git describe --always --tags | sed -e 's,-,\.,g' -e 's,^v,,'); \
	git archive --format=tar --prefix=coreos-installer-$$version/ HEAD | gzip > coreos-installer-$$version.crate; \
	sed -ie "s,^Version:.*,Version: $$version," rust-coreos-installer-gitlab.spec
	sed -ie 's/^Patch/# Patch/g' rust-coreos-installer-gitlab.spec  # we don't want any downstream patches
	sed -ie 's/^Source1/# Source1/g' rust-coreos-installer-gitlab.spec  # we don't vendorz
	# we already have the specfile, so comment it
	spectool -g -s 2 rust-coreos-installer-gitlab.spec  # fetch coreos-installer-dracut just to satisfy rpmbuild
	rpmbuild -bb --define "_sourcedir ${PWD}" --define "_specdir ${PWD}" --define "_builddir ${PWD}" --define "_srcrpmdir ${PWD}" --define "_rpmdir ${PWD}" --define "_buildrootdir ${PWD}/.build" rust-coreos-installer-gitlab.spec
	mv *.src.rpm $$outdir
