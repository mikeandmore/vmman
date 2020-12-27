#!/bin/sh

function RDBYTE() {
    let "b = $RANDOM % 256"
    printf '%.2x' $b
}

if [ $# -eq 0 ]; then
    echo "Usage: $0 vmname"
    echo "Usage: $0 vmname imgfile"
    exit
fi

HOSTIFNAME="eno3"
VMNAME=$1
IFNAME="$USER$VMNAME"
SERIALPATH="/tmp/$USER-$VMNAME.serial"
MACSUFFIX="`RDBYTE`:`RDBYTE`:`RDBYTE`:`RDBYTE`"
IMG="/local/$USER/vm/your_img_here.img"
ISOIMG="/local/$USER/vm/your_installation_iso_here"

if [ $# -eq 2 ]; then
    IMG=$2
    ISOIMG=""
fi

cat <<EOF > ${VMNAME}.toml
[base.system]
machine = "q35,kernel_irqchip=on"
cpu = "host,-vmx,kvm=off,hypervisor=false"
smp = "sockets=1,cores=4,threads=1"
mem = "4096"
mempath = "/dev/hugepages"
smbios = "type=2"
serial = "unix:${SERIALPATH},nowait"
[bridge.${IFNAME}]
interface = "${IFNAME}"
host-interface = "${HOSTIFNAME}"
mac = "BE:EF:${MACSUFFIX}"
driver = "virtio-net-pci"
[storage.main]
driver = "virtio"
file = "${IMG}"
EOF

if [ -n $ISOIMG ]; then
    cat <<EOF >> ${VMNAME}.toml
[storage.cdrom]
driver = "ide"
file = "${ISOIMG}"
media = "cdrom"
EOF
fi
