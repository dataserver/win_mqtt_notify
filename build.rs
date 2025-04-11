extern crate embed_resource;
use copy_to_output::copy_to_output;  
use std::env;

fn main() {
    let _ = embed_resource::compile("resources.rc", embed_resource::NONE);

    // COPY RESOURCES: 
    println!("cargo:rerun-if-changed=images/*");
    copy_to_output("images", &env::var("PROFILE").unwrap()).expect("Could not copy");  
    println!("cargo:rerun-if-changed=config/*");
    copy_to_output("config", &env::var("PROFILE").unwrap()).expect("Could not copy");  
}
