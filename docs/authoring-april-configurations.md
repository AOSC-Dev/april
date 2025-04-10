# Authoring APRIL Configuration Files

AOSC Package Reconstruction Information Listing (or APRIL for short) is
a data description format for semi-automatically working around
installation issues from questionably packaged `.deb` packages.

## Basic Format

Although the APRIL full-listing file is distributed as a JSON-formatted
file, developers wishing to write APRIL configuration files can write
easy TOML format files.

To get an idea of the TOML syntax, you can read a short rundown of the
TOML format here: <https://toml.io/en/>.

To start, you will need to declare the package to patch:

```toml
schema = "0"  # APRIL schema version, must be "0" for now
name = "package"  # name of the package to patch
compatible_versions = "*"  # which version of the package is compatible with your configuration file
```

### Version Matching Syntax

The version-matching syntax should be flexible enough to handle most
scenarios.

The simplest matching syntax is `"*"`, which means "any version." Thus,
your configuration is suitable for any version of the specified package.

If, for example, you want your configuration file to only match the
package of version `1.0.0` till `1.1.0`, then you can write `">= 1.0.0 && < 1.1.0"`.

If the package does not change its version number after its contents
change, you can also use `sha256sum(...)` to match the package. For
example, `sha256sum(0000abcd) || 2.0.0` will match a package with the
SHA256 sum of `0000abcd` or a package with version `2.0.0`.

You can also use the `==` operator to match the version number
precisely. For example, `== 2.1.1+b3` will only match the package with
precisely the version `2.1.1+b3`.

### Total Conversion

If a package is so problematic that you want to discard all its
metadata, you can specify `total_conversion = true` to delete all the
package metadata.

## Overrides

Overrides are how you “correct” or “fix” the erroneous data in the
package.

The following basic overrides are possible:

- `name`: Re-name the package
- `version`: Adjust the version of the package
- `arch`: Re-specify the compatible architecture of the package
- `essential`: Re-specify whether the package is critical to the system
  (i.e. should the user be barred from removing this package?)
- `installed_size`: (int) Specify the installation size of the package
  (how much space the package will take upon installing it)
- `section`: Re-specify which section this package should belong to
- `description`: Re-do the description of the package

The following array overrides are possible:

- `depends`: Adjust the package mandatory dependency
- `recommends`: Adjust the package recommended dependency
- `suggests`: Adjust the package suggested dependency
- `pre_depends`: Adjust the package pre-install dependency (rarely used)
- `breaks`: Adjust the package breakages (which package will this
  package break when installed?)
- `conflicts`: Adjust the package conflicts (which package will this
  package be conflict with when this package is installed?)
- `replaces`: Adjust the package replacements (which package will this
  package be replacing when installed?)
- `provides`: Adjust the package provides (which other package will this
  package be providing/bundling?)

For array overrides, you can do differential replacements for each
definition. For example, if a package contains `a b c` three
dependencies and you want to remove `b` from the dependencies, you can
write `depends = ["-b"]` instead of `depends = ["a", "c"]`. And if
you want to, say, add `d` while removing `c` from the dependencies, you
can instead write `depends = ["+d", "-c"]`.

### Overriding Installation Scripts

Sometimes, it is necessary to replace installation scripts in the
package to ensure compatibility with AOSC OS.

The following installation scripts can be replaced:

- `prerm`: Pre-removal script. It runs before the actual uninstallation
  process.
- `preinst`: Pre-installation script. It runs before the actual
  installation process.
- `postrm`: Post-removal script. It runs after the uninstallation
  process has finished.
- `postinst`: Post-installation script. It runs after the unpacking
  process has finished.
- `triggers`: Triggers definitions. Defines if the installation scripts
  from this package should be run after other packages have file
  changes.

`prerm`, `preinst`, `postrm`, and `postinst` should all be Bash scripts,
while `triggers` scripts should follow the format described in the
Debian Policy Manual:
<https://manpages.debian.org/bookworm/dpkg-dev/deb-triggers.5.en.html>.

### Overriding Configuration Files List

Some packages might not declare their configuration files correctly and
may cause `dpkg` to improperly overwrite the user's changes when the
package gets re-installed or upgraded.

APRIL offers a simple fix for this issue by defining `conffiles = ["path/to/config/file"]`.

## Moving Files Around

Moving files around for some packages may be necessary to fix filesystem
layout issues.

You can define file operations under the `files` table to achieve this.

The following file operations are possible:

- `remove`: Delete specified file.
- `move`: Move the specified file to another location (in the `arg`
  parameter).
- `copy`: Copy the specified file to another location (in the `arg`
  parameter).
- `link`: Symlink specified the file for another location (in the `arg`
  parameter).
- `patch`: Apply a text-based patch to a specified file (in the `arg`
  parameter).
- `binary-patch`: Apply an xdelta3 encoded binary patch to a specified
  file (in the `arg` parameter).
- `track`: Mark the specified file as dpkg-managed (meaning the file
  will be deleted upon uninstallation).
- `add`: Create a new file at the specified location (will fail if the
  file already exists) using specified contents (in the `arg`
  parameter).
- `overwrite`: Same as `add` but will overwrite the file if the file
  already exists.
- `chmod`: Change the permission bits of the specified file (new
  permission bits defined in the `arg` parameter).
- `mkdir`: Create a new directory at the specified location.

By default, all file operations cause the new file to be tracked by
`dpkg`. Currently, there is no way to "untrack" a file to avoid
mistakes.

Optionally, you can also define the `phase` parameter to control when
the file operation will happen. Currently, only `unpack` (after `dpkg`
extracts the files) and `postinst` (after `dpkg` runs the `postinst`
script) are supported.

## Working Example

You can find a commented full example below to show how to use APRIL:

```toml
schema = "0"
name = "sunloginclient"
compatible_versions = "*"

[overrides.scripts]
# we override the post-install script to avoid the stock post-install
# throwing "unsupported OS error"
postinst = """#!/bin/bash
#kill all runing sunloginclient
killall sunloginclient > /dev/null 2>&1

#clear log files
if [ -d '/var/log/sunlogin' ]; then
  rm -rf /var/log/sunlogin
fi
install -dm777 /var/log/sunlogin
chmod 666 /usr/local/sunlogin/res/skin/*.skin
chmod 766 /usr/share/applications/sunlogin.desktop
chmod 644 /usr/local/sunlogin/res/icon/*
systemctl enable runsunloginclient.service --now
"""
# we add `/etc/orayconfig.conf` as a configuration file
# to avoid dpkg overwriting the file when user does an upgrade
conffiles = ["/etc/orayconfig.conf"]
prerm = """#!/bin/bash
systemctl disable runsunloginclient.service --now
"""

[files]
# copy the file to the correct location.
# This is akin to `cp /usr/local/sunlogin/scripts/runsunloginclient.service /etc/systemd/system/runsunloginclient.service`
"/usr/local/sunlogin/scripts/runsunloginclient.service" = { action = "copy", arg = "/etc/systemd/system/runsunloginclient.service" }
```