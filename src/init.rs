use std::env;

pub mod vm;

fn main() {
    let mut mgr = vm::VmManager::new();
    let args = env::args().collect::<Vec<_>>();
    let conf = vm::load_vm(&mut mgr, &args);
    conf.init();
}
