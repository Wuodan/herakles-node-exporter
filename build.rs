// build.rs
use std::env;

fn main() {
    // Generate build info
    vergen::EmitBuilder::builder()
        .all_build()
        .all_git()
        .emit()
        .expect("Unable to generate build info");

    // Check if ebpf feature is enabled
    if env::var("CARGO_FEATURE_EBPF").is_ok() {
        compile_ebpf_programs();
    }
}

#[cfg(feature = "ebpf")]
fn compile_ebpf_programs() {
    use std::path::PathBuf;
    use std::process::Command;

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bpf_src = PathBuf::from("src/ebpf/bpf");

    // Check for required tools
    check_tool("clang", "--version");

    println!("cargo:rerun-if-changed=src/ebpf/bpf/process_io.bpf.c");

    // Find libbpf headers from libbpf-sys
    let libbpf_include = find_libbpf_include_dir();

    // Compile eBPF program with better error output
    let bpf_obj = out_dir.join("process_io.bpf.o");
    let bpf_c_file = bpf_src.join("process_io.bpf.c");

    let mut clang_args = vec![
        "-g".to_string(),
        "-O2".to_string(),
        "-target".to_string(),
        "bpf".to_string(),
        "-D__TARGET_ARCH_x86".to_string(),
        "-D__BPF_TRACING__".to_string(),
        "-I".to_string(),
        bpf_src.to_str().unwrap().to_string(),
    ];

    // Add libbpf include path if found
    if let Some(libbpf_path) = libbpf_include {
        clang_args.push("-I".to_string());
        clang_args.push(libbpf_path);
    }
    clang_args.push("-c".to_string());
    clang_args.push(bpf_c_file.to_str().unwrap().to_string());
    clang_args.push("-o".to_string());
    clang_args.push(bpf_obj.to_str().unwrap().to_string());

    let output = Command::new("clang")
        .args(&clang_args)
        .output()
        .expect("Failed to execute clang");

    if !output.status.success() {
        eprintln!("=== eBPF Compilation Failed ===");
        eprintln!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
        eprintln!("===============================");
        panic!("eBPF compilation failed. See output above for details.");
    }

    println!(
        "cargo:warning=✅ eBPF object built at: {}",
        bpf_obj.display()
    );

    fn check_tool(tool: &str, arg: &str) {
        let output = Command::new(tool).arg(arg).output();

        match output {
            Ok(out) if out.status.success() => {
                println!("cargo:warning=Found {}: OK", tool);
            }
            _ => {
                panic!(
                    "{} not found or failed. Required for eBPF compilation.",
                    tool
                );
            }
        }
    }

    fn find_libbpf_include_dir() -> Option<String> {
        // libbpf-sys will build libbpf and put headers in OUT_DIR/include
        // We need to find the libbpf-sys OUT_DIR
        let out_dir = env::var("OUT_DIR").unwrap();
        let out_path = PathBuf::from(&out_dir);

        // Navigate up to target/release/build or target/debug/build
        if let Some(build_dir) = out_path
            .ancestors()
            .find(|p| p.file_name().map_or(false, |n| n == "build"))
        {
            // Find libbpf-sys-* directory
            if let Ok(entries) = std::fs::read_dir(build_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("libbpf-sys-")
                    {
                        let include_dir = path.join("out").join("include");
                        if include_dir.exists() {
                            println!(
                                "cargo:warning=Found libbpf headers at: {}",
                                include_dir.display()
                            );
                            return Some(include_dir.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        // Fallback: try system headers
        for path in &["/usr/include", "/usr/local/include"] {
            let bpf_helpers = PathBuf::from(path).join("bpf/bpf_helpers.h");
            if bpf_helpers.exists() {
                println!("cargo:warning=Using system libbpf headers at: {}", path);
                return Some(path.to_string());
            }
        }

        println!("cargo:warning=Could not find libbpf headers, compilation may fail");
        None
    }
}

#[cfg(not(feature = "ebpf"))]
fn compile_ebpf_programs() {}
