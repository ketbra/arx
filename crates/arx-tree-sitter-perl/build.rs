// Compile the vendored tree-sitter-perl grammar (parser.c + scanner.c)
// into a static library that we link against from lib.rs.
//
// We avoid depending on the upstream `tree-sitter-perl` crate because
// it pulls in tree-sitter 0.26, which conflicts with the rest of our
// workspace's tree-sitter 0.25 (only one `links = "tree-sitter"`
// crate is allowed in a dependency graph). Vendoring lets us compile
// the same grammar against any tree-sitter runtime that supports the
// stable LanguageFn ABI.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(src_dir);

    // The vendored grammar has some benign "may be used uninitialized"
    // warnings in its scanner.c — suppress them so cargo's output
    // stays clean. These don't affect correctness.
    c_config.warnings(false);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    let scanner_path = src_dir.join("scanner.c");
    if scanner_path.exists() {
        c_config.file(&scanner_path);
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    }

    c_config.compile("arx-tree-sitter-perl");
}
