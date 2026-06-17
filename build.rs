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
    println!("cargo:rerun-if-changed=src/ebpf/bpf/vmlinux.h");

    // Find libbpf headers from libbpf-sys
    let libbpf_include = find_libbpf_include_dir();
    let generated_vmlinux_h = out_dir.join("vmlinux.h");
    let vmlinux_include_dir = prepare_vmlinux_header(&bpf_src, &generated_vmlinux_h);

    // Compile eBPF program with better error output
    let bpf_obj = out_dir.join("process_io.bpf.o");
    let bpf_c_file = bpf_src.join("process_io.bpf.c");

    let mut clang_args = vec![
        "-g".to_string(),
        "-O2".to_string(),
        "-target".to_string(),
        "bpf".to_string(),
        "-D__TARGET_ARCH_x86".to_string(),
        "-D__BPF_TRACING__".to_string(), // Important for BPF_CORE_READ macros
        "-I".to_string(),
        vmlinux_include_dir.to_str().unwrap().to_string(),
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

    // Copy the compiled eBPF object to src tree for embedding with include_bytes!()
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let embedded_obj = manifest_dir.join("src/ebpf/bpf/process_io.bpf.o");
    std::fs::copy(&bpf_obj, &embedded_obj).expect("Failed to copy eBPF object to src tree");

    println!(
        "cargo:warning=✅ eBPF object embedded at: {}",
        embedded_obj.display()
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

    fn prepare_vmlinux_header(bpf_src: &std::path::Path, output_path: &std::path::Path) -> PathBuf {
        let checked_in = bpf_src.join("vmlinux.h");
        let btf_path = "/sys/kernel/btf/vmlinux";

        let header_contents = if std::path::Path::new(btf_path).exists() {
            match Command::new("bpftool")
                .args(["btf", "dump", "file", btf_path, "format", "c"])
                .output()
            {
                Ok(output) if output.status.success() => {
                    println!(
                        "cargo:warning=Using freshly generated vmlinux.h at: {}",
                        output_path.display()
                    );
                    String::from_utf8(output.stdout)
                        .expect("Generated vmlinux.h is not valid UTF-8")
                }
                Ok(_) => panic!("Failed to generate vmlinux.h from kernel BTF."),
                Err(err) => {
                    println!(
                        "cargo:warning=bpftool unavailable ({}). Using checked-in vmlinux.h",
                        err
                    );
                    std::fs::read_to_string(&checked_in)
                        .expect("Checked-in vmlinux.h is missing or unreadable")
                }
            }
        } else {
            println!(
                "cargo:warning=Kernel BTF not found at {}. Using checked-in vmlinux.h",
                btf_path
            );
            std::fs::read_to_string(&checked_in)
                .expect("Checked-in vmlinux.h is missing or unreadable")
        };

        let sanitized = sanitize_vmlinux_header(&header_contents);
        std::fs::write(output_path, sanitized).expect("Failed to write prepared vmlinux.h");
        output_path
            .parent()
            .expect("Prepared vmlinux.h has no parent directory")
            .to_path_buf()
    }

    fn sanitize_vmlinux_header(contents: &str) -> String {
        let mut sanitized = String::with_capacity(contents.len());
        for line in contents.lines() {
            if line.starts_with(
                "extern int bpf_stream_vprintk(int stream_id, const char *fmt__str, const void *args, u32 len__sz) __weak __ksym;"
            ) {
                println!(
                    "cargo:warning=Stripping incompatible bpf_stream_vprintk declaration from vmlinux.h"
                );
                sanitized.push_str("/* stripped incompatible bpf_stream_vprintk declaration */\n");
                continue;
            }
            sanitized.push_str(line);
            sanitized.push('\n');
        }
        if !sanitized.contains("#ifndef") || !sanitized.contains("struct") {
            panic!("Generated vmlinux.h does not appear to be a valid C header");
        }
        sanitized
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
