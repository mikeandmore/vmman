use libc;
use toml::value;
use std::{fs, path, thread::sleep, time};
use std::io::prelude::*;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::process::Command;

pub trait ConfModule {
    fn init(&self, _: u32, _: u32) {}
    fn startup_args(&mut self) -> Vec<String>;
    fn post_startup(&mut self) {}
}

fn get_string(conf: &value::Table, key: &str) -> String {
    let err = format!("Expecting {} as a string", key);
    return String::from(conf.get(key).expect(&err).as_str().expect(&err));
}

fn get_option_string(conf: &value::Table, key: &str) -> Option<String> {
    return conf.get(key).map(|x| x.as_str()).flatten().map(|x| String::from(x));
}

pub fn create_module(heading: &str, section: &value::Table) -> Box<dyn ConfModule> {
    match heading {
	"bridge" => Box::new(MacVTapModule::new(section)),
	"pcie-passthrough" => Box::new(VfioModule {
	    name: get_string(section, "dev"),
	    romfile: get_option_string(section, "romfile"),
	}),
	"base" => Box::new(BaseModule::new(section)),
	"apple-smc" => Box::new(AppleSMCModule {
	    osk: get_string(section, "osk"),
	}),
	"storage" => Box::new(StorageModule {
	    driver: get_string(section, "driver"),
	    filename: get_string(section, "file"),
	    media: get_option_string(section, "media"),
	}),
	x => panic!("unknown module {}", x)
    }
}

fn build_path(prefix: &str, name: &str, suffix: &str) -> path::PathBuf {
    let mut p = path::PathBuf::from(prefix);
    p.push(name);
    p.push(suffix);
    return p;
}

fn init_perm<P: AsRef<path::Path>>(path: &P, uid: u32, gid: u32) {
    println!("Initializing Permissions on {}", path.as_ref().to_str().unwrap());

    let f = fs::File::open(path).expect(&format!("cannot open device")); // shouldn't happen
    
    match f.metadata() {
	Ok(metadata) => {
	    let owner = metadata.uid();
	    let group_owner = metadata.gid();
	    let pathname = path.as_ref().to_str().unwrap();
	    if owner != uid || group_owner != gid {
		println!("  Changing the owner of {} to {}:{}", pathname, uid, gid);
		if unsafe { libc::fchown(f.as_raw_fd(), uid, gid) } != 0 {
		    panic!(format!("failed to set ownership for file {}", pathname));
		}
	    } else {
		println!("  {} is ready.", pathname);
	    }
	}
	Err(_) => {
	    panic!(format!("Cannot access metadata for file {}", path.as_ref().to_str().unwrap()))
	}
    }
}

// Bridge
struct MacVTapModule {
    ifname: String,
    ifhost: String,
    macaddress: String,
    driver: String,

    tapfile: Option<fs::File>,
}

impl MacVTapModule {
    pub fn new(conf: &value::Table) -> MacVTapModule {
	return MacVTapModule {
	    ifname: get_string(conf, "interface"),
	    ifhost: get_string(conf, "host-interface"),
	    macaddress: get_string(conf, "mac"),
	    driver: get_string(conf, "driver"),
	    tapfile: None,
	}
    }

    fn ifidx(&self) -> String {
	return String::from(fs::read_to_string(build_path("/sys/class/net", &self.ifname, "ifindex").as_path()).expect("Cannot read ifindex").trim_end());
    }
}

impl ConfModule for MacVTapModule {
    fn init(&self, uid: u32, gid: u32) {
	let net_class_path = build_path("/sys/class/net", &self.ifname, "");
	if net_class_path.exists() {
	    println!("Link {} exist, removing...", net_class_path.to_str().unwrap());
	    let p = Command::new("ip")
		.args(&["link", "del", &self.ifname])
		.output()
		.expect("Cannot run ip link to delete the old macvtap");
	    if !p.status.success() {
		panic!(String::from_utf8(p.stderr).unwrap());
	    }
	}
	println!("Creating and enabling link {} name {} with mac {}",
		 &self.ifhost, &self.ifname, &self.macaddress);
	{
	    let p = Command::new("ip")
		.args(&["link", "add", "link", &self.ifhost, "name", &self.ifname, "type", "macvtap", "mode", "bridge"])
		.output()
		.expect("Cannot run ip link to create a new macvtap");

	    if !p.status.success() {
		panic!(String::from_utf8(p.stderr).unwrap());
	    }
	}
	{
	    let p = Command::new("ip")
		.args(&["link", "set", &self.ifname, "address", &self.macaddress, "up"])
		.output()
		.expect("Cannot up the link with the mac address");
	    if !p.status.success() {
		panic!(String::from_utf8(p.stderr).unwrap());
	    }
	}

	let net_class_macvtap_path = build_path("/sys/class/net", &self.ifname, "macvtap");
	if !net_class_macvtap_path.exists() {
	    panic!("{} does not exist, {} isn't a macvtap interface!",
		   net_class_macvtap_path.to_str().unwrap(), self.ifname);
	}

	let ifidx = self.ifidx();
	// race here?
	sleep(time::Duration::from_millis(3000));
	init_perm(&format!("/dev/tap{}", ifidx), uid, gid);
    }
    fn startup_args(&mut self) -> Vec<String> {
	let tapfile = fs::OpenOptions::new().read(true).write(true).open(format!("/dev/tap{}", self.ifidx())).expect("Cannot open tap device");
	let fd = tapfile.as_raw_fd();
	self.tapfile = Some(tapfile);

	unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
	}

	return vec![String::from("-netdev"),
		    format!("tap,id={},fd={},vhost=on", &self.ifname, fd),
		    String::from("-device"),
		    format!("{},netdev={},mac={}", &self.driver, &self.ifname, &self.macaddress)];
    }
    fn post_startup(&mut self) {
	// self.tapfile = None;
    }
}

// PCIE passthrough
struct VfioModule {
    name: String,
    romfile: Option<String>,
}

impl VfioModule {
    fn override_driver(&self) {
	println!("Overriding drivers for {}", &self.name);
	{
	    let err = "Cannot override current driver.";
	    let path_buf = build_path("/sys/bus/pci/devices", &self.name, "driver_override");
	    println!("  Overriding in {}", path_buf.to_str().unwrap());
	    let mut f = fs::OpenOptions::new().read(true).write(true).open(path_buf).expect(err);
	    f.write_all(b"vfio-pci").expect(err);
	}
	{
	    let err = "Cannot probe driver after override";
	    let path_buf = "/sys/bus/pci/drivers_probe";
	    println!("  Probing drivers using {}", path_buf);
	    let mut f = fs::OpenOptions::new().read(true).write(true).open(&path_buf).expect(err);
	    f.write_all(self.name.as_bytes()).expect(err);
	}
    }
}

impl ConfModule for VfioModule {
    fn init(&self, uid: u32, gid: u32) {
	let p = fs::read_link(build_path("/sys/bus/pci/devices", &self.name, "driver"));
	if p.is_ok() {
	    let drv_name = String::from(p.unwrap().file_name().unwrap().to_str().unwrap());
	    if &drv_name != "vfio-pci" {
		println!("Unbinding PCI device {} from driver {}", &self.name, &drv_name);
		{
		    let err = "Cannot unbind device";
		    let mut f = fs::OpenOptions::new().read(true).write(true).open(build_path("/sys/bus/pci/devices", &self.name, "driver/unbind")).expect(err);
		    f.write_all(self.name.as_bytes()).expect(err);
		}
		self.override_driver()
	    }
	} else {
	    self.override_driver()
	}
	let err = "Cannot read IOMMU Group ID";
	let iommu_group_path = fs::read_link(build_path("/sys/bus/pci/devices", &self.name, "iommu_group")).expect(err);
	let iommu_group = iommu_group_path.file_name()
	    .expect(err)
	    .to_str()
	    .expect(err);
	println!("PCI device {} is under IOMMU Group {}", &self.name, iommu_group);
	init_perm(&format!("/dev/vfio/{}", iommu_group), uid, gid);
    }

    fn startup_args(&mut self) -> Vec<String> {
	let mut detail = format!("vfio-pci,host={}", &self.name);
	if self.romfile.is_some() {
	    detail += &format!(",romfile={},multifunction=on", self.romfile.as_ref().unwrap());
	}
	return vec![String::from("-device"), detail]
    }
}

struct BaseModule {
    machine: String,
    cpu: Option<String>,
    smp: String,
    mem: String,
    mempath: Option<String>,
    smbios: Option<String>,
    vga: Option<String>,
    display: Option<String>,
    serial: Option<String>,
}

impl BaseModule {
    fn new(conf: &value::Table) -> BaseModule {
	return BaseModule {
	    machine: get_string(conf, "machine"),
	    cpu: get_option_string(conf, "cpu"),
	    smp: get_string(conf, "smp"),
	    mem: get_string(conf, "mem"),
	    mempath: get_option_string(conf, "mepmath"),
	    smbios: get_option_string(conf, "smbios"),
	    vga: get_option_string(conf, "vga"),
	    display: get_option_string(conf, "display"),
	    serial: get_option_string(conf, "serial"),
	}
    }
}

impl ConfModule for BaseModule {
    fn startup_args(&mut self) -> Vec<String> {
	let mut r = vec![String::from("-machine"), self.machine.clone(),
			 String::from("-smp"), self.smp.clone(),
			 String::from("-m"), self.mem.clone(),
			 String::from("-mem-path"), String::from("/dev/hugepages")];
	if self.mempath.is_some() {
	    r.push(String::from("-mem-path"));
	    r.push(String::from(self.mempath.as_ref().unwrap()));
	}
	if self.cpu.is_some() {
	    r.push(String::from("-cpu"));
	    r.push(String::from(self.cpu.as_ref().unwrap()));
	}
	if self.smbios.is_some() {
	    r.push(String::from("-smbios"));
	    r.push(String::from(self.smbios.as_ref().unwrap()));
	}
	if self.serial.is_some() {
	    r.push(String::from("-serial"));
	    r.push(String::from(self.serial.as_ref().unwrap()));
	}
	r.push(String::from("-vga"));
	r.push(String::from(self.vga.as_ref().map(|s| s.as_str()).unwrap_or("none")));
	r.push(String::from("-display"));
	r.push(String::from(self.display.as_ref().map(|s| s.as_str()).unwrap_or("none")));
	return r;
    }
}

// Apple SMC
struct AppleSMCModule {
    osk: String,
}

impl ConfModule for AppleSMCModule {
    fn startup_args(&mut self) -> Vec<String> {
	return vec![String::from("-device"), format!("isa-applesmc,osk={}", &self.osk)];
    }
}

struct StorageModule {
    driver: String,
    filename: String,
    media: Option<String>,
}

impl ConfModule for StorageModule {
    fn startup_args(&mut self) -> Vec<String> {
	return vec![String::from("-drive"),
		    format!("if={},format=raw,aio=native,cache.direct=on,file={}{}",
			    &self.driver, &self.filename,
			    self.media.as_ref().map(|s| String::from(",media=") + s).unwrap_or(String::from("")))]
    }
}
