use std::{env, fs::{create_dir, remove_dir_all}};
use std::str;
use std::vec::Vec;
use std::fs::File;
use libc;
use quick_xml::{Reader, events::{BytesStart, Event}};
use std::{ffi::CString, io::prelude::*};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicI32, Ordering}};
use curl::easy::Easy;

fn process_ovf_xml_tag(evt: &BytesStart, disk_images: &mut Vec<String>) {
    if evt.name() == b"File" {
	match evt.attributes().find(|ref att| { att.as_ref().unwrap().key == b"ovf:href" }) {
	    Some(a) => {
		// println!("{}", String::from_utf8(a.unwrap().value.to_vec()).unwrap());
		if let Ok(image_name) = String::from_utf8(a.unwrap().value.to_vec()) {
		    disk_images.push(image_name);
		} else {
		    println!("invalid filenames inside the File tag");
		}
	    },
	    None => {
		panic!("Cannot find ovf:href attribute for File tag");
	    }
	}
    }
}

fn parse_ovf(ovf_file: &str) -> Vec<String> {
    println!("Parsing ovf description file {}", ovf_file);
    let mut xml = Reader::from_file(ovf_file).expect(&format!("Cannot open file {}", ovf_file));
    let mut buffer = Vec::new();
    let mut disk_images = Vec::<String>::new();
    loop {
	match xml.read_event(&mut buffer) {
	    Ok(Event::Start(ref evt)) => {
		// println!("event start {}", String::from_utf8(evt.name().to_vec()).unwrap());
		process_ovf_xml_tag(evt, &mut disk_images);
	    },
	    Ok(Event::End(_)) => {
		// println!("event end {}", String::from_utf8(evt.name().to_vec()).unwrap());
	    },
	    Ok(Event::Empty(ref evt)) => {
		// println!("event empty {}", String::from_utf8(evt.name().to_vec()).unwrap());
		process_ovf_xml_tag(evt, &mut disk_images);
	    }
	    Ok(Event::Eof) => {
		break;
	    },
	    Err(_) => {
		panic!("Error parsing xml at {}", xml.buffer_position());
	    },
	    _ => {}
	}
	buffer.clear();
    }

    return disk_images;
}

fn os_system(cmd: String) -> Result<(), ()> {
    if unsafe { libc::system(CString::new(cmd).unwrap().as_ptr()) } != 0 {
	Err(())
    } else {
	Ok(())
    }
}

fn convert_all_images(prefix_name: &str, disk_images: Vec<String>) {
    if disk_images.len() == 1 {
	println!("Converting to {}.img", prefix_name);
	let cmd = format!("qemu-img convert -O raw {}/{} {}.img", prefix_name, disk_images[0], prefix_name);
	os_system(cmd).expect("Qemu fail to convert image");
    } else {
	let mut count = 0;
	disk_images.into_iter().for_each(|ref filename| {
	    println!("Converting to {}-{}.img", prefix_name, count);
	    let cmd = format!("qemu-img convert -O raw {}/{} {}-{}.img", prefix_name, filename, prefix_name, count);
	    os_system(cmd).expect("Qemu fail to convert image");
	    count += 1;
	})
    }
}

fn download_and_convert_image(vendor: &str, name: &str, url: &str) -> Result<(), String> {
    let prefix_name = Arc::new(format!("{}-{}", vendor, name));
    let prefix_name_ref = prefix_name.clone();
    let old_panic_hook = std::panic::take_hook();
    
    std::panic::set_hook(Box::new(move |info| {
	if let Some(msg) = info.payload().downcast_ref::<String>() {
	    println!("Error: {}", msg);
	}
	println!("Removing working dir {}", prefix_name_ref.as_ref());
	remove_dir_all(prefix_name_ref.as_ref()).unwrap();
	old_panic_hook(info);
    }));
    
    let filename = format!("{}/ovf", &prefix_name);
    println!("Saving temporary files to {}/", &prefix_name);

    let mut easy = Easy::new();
    easy.url(url).unwrap();
    easy.follow_location(true).unwrap();
    easy.progress(true).unwrap();
    
    create_dir(prefix_name.as_ref()).expect(
	&format!("Cannot create dir {}. Another pull with the same name in progress/failed?",
		 prefix_name.as_ref()));
    
    let mut transfer = easy.transfer();
    
    let f = Arc::new(Mutex::new(File::create(&filename).expect("Cannot open file")));

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
	if let Ok(_) = fref.lock().unwrap().write(data) {} else {
	    has_error_ref.store(true, Ordering::Release);
	};
	
	Ok(data.len())
    }).unwrap();
    
    match transfer.perform() {
	Ok(_) => {
	    if has_error.load(Ordering::Acquire) {
		Err("Download error".to_string())
	    } else {
		let cmd = format!("tar xf {}/ovf -C {}/", prefix_name.as_ref(), prefix_name.as_ref());
		os_system(cmd).expect("Extracting box image failed");
		// println!("{}", cmd);
		convert_all_images(
		    prefix_name.as_ref(),
		    parse_ovf(&format!("{}/box.ovf", prefix_name.as_ref())));
		remove_dir_all(prefix_name.as_ref()).unwrap();
		Ok(())
	    }
	}
	Err(_) => {
	    remove_dir_all(prefix_name.as_ref()).unwrap();
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
