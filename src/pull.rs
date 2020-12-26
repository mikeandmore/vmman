use std::env;
use std::str;
use std::vec::Vec;
use std::fs::File;
use libc;
use std::ffi::CString;
use std::io::prelude::*;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use curl::easy::Easy;

fn download_and_convert_image(vendor: &str, name: &str, url: &str) -> Result<(), String> {
    let mut easy = Easy::new();
    easy.url(url).unwrap();
    easy.follow_location(true).unwrap();
    easy.progress(true).unwrap();

    let prefix_name = format!("{}-{}", vendor, name);
    let mut transfer = easy.transfer();
    let filename = format!("{}.vdi", prefix_name);
    
    let f = Arc::new(Mutex::new(
	match File::create(filename) {
	    Ok(f) => {f}
	    Err(_) => { return Err("Cannot open file".to_string()); }
	}
    ));

    let has_error = Arc::new(AtomicBool::new(false));
    let has_error_ref = has_error.clone();

    let last_dlnow = AtomicI32::new(0);
            
    transfer.progress_function(move |dltot, dlnow, _, _| {
	let t = (dlnow / 1024. / 1024.) as i32;
	if t > last_dlnow.load(Ordering::Acquire) {
	    last_dlnow.store(t, Ordering::Release);
	    print!("Download Progress {:6}M/{:6}M\r", t, (dltot / 1024. / 1024.) as i32);
	    std::io::stdout().flush().unwrap();
	}
	true
    }).unwrap();

    let fref = f.clone();
    transfer.write_function(move |data| {
	match fref.lock().unwrap().write(data) {
	    Ok(_) => {}
	    Err(_) => { has_error_ref.store(true, Ordering::Release); }
	};
	
	Ok(data.len())
    }).unwrap();
    
    match transfer.perform() {
	Ok(_) => {
	    if has_error.load(Ordering::Acquire) {
		Err("Download error".to_string())
	    } else {
		let cmd = format!("qemu-img convert -f virtualbox -O raw {}.vdi {}.img", prefix_name, prefix_name);
		unsafe { libc::system(CString::new(cmd).unwrap().as_ptr()); }
		// println!("{}", cmd);
		Ok(())
	    }
	}
	Err(_) => {
	    Err("Cannot download image file".to_string())
	}
    }
}

fn parse_repo_name<'a>(repo_name: &'a str) -> (&'a str, &'a str) {
    let name_vec = repo_name.split("/").collect::<Vec<&str>>();
    if name_vec.len() != 2 {
	panic!("Invalid repo name {}", repo_name);
    }

    let vendor = name_vec[0];
    let name = name_vec[1];
    
    (vendor, name)
}

fn pull_from_url(vendor: &str, name: &str, url: &str) {
    let mut easy = Easy::new();
    let dst = Arc::new(Mutex::new(Vec::new()));
    let dst_closure = dst.clone();
    easy.url(url.as_ref()).unwrap();
    let mut transfer = easy.transfer();
    
    transfer.write_function(|data| {
	dst_closure.lock().unwrap().extend_from_slice(data);
	Ok(data.len())
    }).unwrap();
    transfer.perform().unwrap();
    println!("");

    let jobj = json::parse(str::from_utf8(dst.lock().unwrap().as_ref()).unwrap());
    match jobj {
	Ok(x) => {
	    let latest_providers = &x["versions"][0]["providers"];
	    if latest_providers.is_null() {
		println!("Cannot find the latest version");
		return;
	    }
	    let (parsed_vendor, parsed_name) =
		parse_repo_name(x["name"].as_str().unwrap());
	    
	    if parsed_vendor != vendor || parsed_name != name {
		println!("{} is inconsistent with {}/{}",
			 latest_providers["name"].as_str().unwrap(),
			 vendor, name);
		return;
	    }

	    for provider in latest_providers.members() {
		if provider["name"] == "virtualbox" {
		    let image_url = provider["url"].as_str().unwrap();
		    println!("Downloading {}", image_url);
		    match download_and_convert_image(vendor, name, image_url) {
			Ok(_) => {
			    println!("Pulled {}/{}", vendor, name);
			}
			Err(_) => {
			    println!("Failed to pull {}/{}", vendor, name);
			}
		    }
		    break;
		}
	    }
	},
	Err(_) => {
	    println!("Cannot parse {}", url);
	}
    }
}

fn pull_from_vagrant(vendor: &str, name: &str) {
    let url = format!("https://app.vagrantup.com/{}/boxes/{}", vendor, name);
    pull_from_url(vendor, name, url.as_ref());
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let (vendor, name) = parse_repo_name(args[0].as_ref());
	
    if args.len() == 1 { 
	pull_from_vagrant(vendor, name);
    } else {
	pull_from_url(vendor, name, args[1].as_ref());
    }
}
