use std::{env, fs};
use toml::value;
use std::os::unix::fs::MetadataExt;
use std::collections::BTreeMap;
use std::process::Command;

#[path = "modules.rs"] mod modules;

pub struct VmConf {
    pub name: String,
    pub filename: String,
    uid: u32,
    gid: u32,
    pub modules: Vec<Box<dyn modules::ConfModule>>,
}

pub struct VmManager {
    pub vmconfs: BTreeMap<String, VmConf>,
}

impl VmConf {
    pub fn new(name: &str, filename: &str) -> VmConf {
	VmConf {
	    name: String::from(name),
	    filename: String::from(filename),
	    uid: 0,
	    gid: 0,
	    modules: Vec::<Box<dyn modules::ConfModule>>::new(),
	}
    }

    pub fn load(self: &mut Self) {
	let conf_file_content = fs::read_to_string(&self.filename).expect("Cannot read file");
	let conf = toml::from_str::<value::Table>(&conf_file_content).expect("Cannot parse configuration");
	let md = fs::File::open(&self.filename).unwrap().metadata().expect("Cannot get metadata of the configuration file");
	self.uid = md.uid();
	self.gid = md.gid();

	for (module_name, sections) in conf {
	    for (_, section) in sections.as_table().expect("Section must have names") {
		self.modules.push(modules::create_module(&module_name, section.as_table().expect("Section must have names")));
	    }
	}
    }

    pub fn init(self: &mut Self) {
	println!("Using the permission from the configuration file to init resources. uid = {}, gid = {}",
		 self.uid, self.gid);
	for m in self.modules.iter() {
	    m.init(self.uid, self.gid);
	}
    }
    pub fn run(self: &mut Self) {
	let mut args = vec![String::from("-enable-kvm")];
	for m in self.modules.iter_mut() {
	    args.extend(m.startup_args());
	}
	for a in &args {
	    print!("{} ", &a);
	}
	println!("");

	let qemubin = env::var("QEMU_BIN").unwrap_or(String::from("/usr/bin/qemu-system-x86_64"));
	
	let p = Command::new(&qemubin)
	    .args(args)
	    .spawn().expect("Cannot spawn qemu process");
	println!("Qemu started with {}", p.id());
	for m in self.modules.iter_mut() {
	    m.post_startup();
	}
    }
}

impl VmManager {
    pub fn new() -> VmManager {
	let confdir = env::var("VMCONF_DIR")
	    .unwrap_or(env::var("HOME").expect("Cannot get home dir from the system!") + "/vm");
	let dir = fs::read_dir(&confdir).expect(&format!("Cannot open vm configuration dir {}", &confdir));
	let err = "Cannot decode filename";

	let vmconfs =
	    dir.filter_map(
		|e| {
		    match e {
			Ok(entry) => {
			    if let Ok(t) = entry.file_type() {
				if !t.is_file() {
				    return None;
				}
			    } else {
				return None;
			    }
			    
			    if entry.path().extension().expect(err).to_str() == Some("toml") {
				return Some(entry.path());
			    } else {
				return None;
			    }
			},
			Err(_) => None
		    }
		}
	    ).map(
		|p| {
		    let name: &str = p.file_stem().expect(err).to_str().expect(err);
		    let filename: &str = p.to_str().expect(err);
		    (String::from(name), VmConf::new(name, filename))
		}
	    ).collect::<BTreeMap<_, _>>();

	VmManager {
	    vmconfs: vmconfs,
	}
    }
}

pub fn load_vm<'a>(mgr: &'a mut VmManager, args: &Vec<String>) -> &'a mut VmConf {
    let target: &str = args.get(1).expect("Expecting a machine name");
    let conf = mgr.vmconfs.get_mut(target).expect(&format!("Cannot find machine {}", target));
    conf.load();
    return conf;
}
