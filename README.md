# Usage

Virtual machines are described using toml files, and they are stored under `$VMCONF_DIR`. To create a virtual machine, you can run the `new.sh` utility. You may customize the `new.sh` script according to your infrastructure. You can use the `vm-list` command to list all virtual machines.

To initialize a VM, run `vm-init`. For example:

	# vm-init my-windows-vm

This will assign system resources to this VM, according to the description of the toml file. To do this, you need to run `vm-init` as root. In practice, you can either run using `sudo`, or make `vm-init` a SUID binary. After this, simply run

	$ vm-run my-windows-vm

`vm-run` will launch qemu (according to `$QEMU_BIN` variable or `/usr/bin/qemu-system-x86_64` by default) as the current user (non-root).

You may install your own OS inside the VM, but more often, you want to pull a VM image from Vagrant or another custom location. You can do this with `vm-pull`. For example, both these commands are supported.

	$ vm-pull ubuntu/trusty64
	$ vm-pull oraclelinux/8 https://oracle.github.io/vagrant-projects/boxes/oraclelinux/8.json

# Why

Why there is this project? Why not libvirt? libvirt has two major flaws.

First, libvirt has its own infrastructure conventions. For example, all VMs are under the same "network", which is really a bridge interface that libvirt setup. All VM networks are managed by libvirt's own `dnsmasq` dhcp server. All storages are stored in a pool, which is really a directory or a remote resource that libvirt setups. However, in practice, these infrastructures already exist and they are very different from libvirt's convention. In our cluster, we have a centralized NFS server for each user, so each user can store their VM images there. We have a centralized `dhcpd` server for the entire cluster, and we would like to use the setting there for the VM as well.

Second, libvirt works well for all functions it supports, which is a very small subset of Qemu. For the functionality it doesn't support, users have to modify the xml file by hand. In practice, we constantly use these features, for example, PCI passthrough with custom ROM, or disable `hv_vendor_id` for NVIDIA driver.

vmman is a simple Qemu launcher with no resource management daemon. It does not manage VM resources other than creating tap interfaces. Users can reuse their storage and network infrastructures, for example, existing NFS or bridge setups. vmman provides a `vm-init` to create tap interfaces and assign system resources to non-root users so that VM can run without root. This not only enhances security but also make the VM process killable by the user who started them.

# Why Rust

In practice, we would like to have any regular user to run `vm-init`, which is either a SUID binary or sudo runnable command. However, user can maliciously construct a toml file such that vm-init can do anything for them as root. Rust is a memory safe language and the compiler is a good tool to mitigate this issue.
