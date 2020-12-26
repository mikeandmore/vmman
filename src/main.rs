use std::env;
pub mod vm;

fn usage() {
    println!("Cannot run vmman binary directly. Please run vm-run/vm-init/vm-list.");
}

fn main() {
    let args = env::args().collect::<Vec<_>>();

    match args.get(0) {
	Some(x) => {
	    let mut mgr = vm::VmManager::new();
	    match x.as_str() {
		"vm-list" => {
		    for (_, x) in mgr.vmconfs.iter() {
			println!("  {} in {}", x.name, x.filename)
		    }
		}
		"vm-run" => {
		    let conf = vm::load_vm(&mut mgr, &args);
		    conf.run()
		}
		_ => {
		    usage();
		    return;
		}
	    }
	},
	None => {
	    usage();
	    return;
	}
    }
}
