use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const RUNNER_C_CHUNKS: &'static str = include_str!("../../wasm/chunks.c");
const RUNNER_C_INDEX: &'static str = include_str!("../../wasm/index.c");
const RUNNER_C_ROARING: &'static str = include_str!("../../wasm/roaring.c");
const RUNNER_C_SYS: &'static str = include_str!("../../wasm/sys.c");

#[allow(dead_code)]
pub enum WasmStandard {
    C89,
    C99,
    C11,
    C17,
}

#[allow(dead_code)]
pub enum WasmOptimisationLevel {
    Level(u8),
    Fast,
    S,
    Z,
    G,
}

#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum WasmCompileWarning {
    UnusedFunction,
}

pub struct WasmCompileArgs<'iw, 'm, 'i, 'o> {
    standard: WasmStandard,
    optimisation_level: WasmOptimisationLevel,
    all_warnings: bool,
    extra_warnings: bool,
    warnings_as_errors: bool,
    ignore_warnings: &'iw [WasmCompileWarning],
    macros: &'m [(&'m str, &'m str)],
    input: &'i PathBuf,
    output: &'o PathBuf,
}

pub fn compile_to_wasm(WasmCompileArgs {
    standard,
    optimisation_level,
    all_warnings,
    extra_warnings,
    warnings_as_errors,
    ignore_warnings,
    macros,
    input,
    output,
}: WasmCompileArgs) -> () {
    let mut cmd = Command::new("clang");
    cmd.arg(format!("-std={}", match standard {
        WasmStandard::C89 => "c89",
        WasmStandard::C99 => "c99",
        WasmStandard::C11 => "c11",
        WasmStandard::C17 => "c17",
    }).as_str());
    cmd.arg(format!("-O{}", match optimisation_level {
        WasmOptimisationLevel::Level(0) => "0",
        WasmOptimisationLevel::Level(1) => "1",
        WasmOptimisationLevel::Level(2) => "2",
        WasmOptimisationLevel::Level(3) => "3",
        WasmOptimisationLevel::Fast => "fast",
        WasmOptimisationLevel::S => "s",
        WasmOptimisationLevel::Z => "z",
        WasmOptimisationLevel::G => "g",
        _ => panic!("Invalid optimisation level"),
    }).as_str());
    if all_warnings { cmd.arg("-Wall"); };
    if extra_warnings { cmd.arg("-Wextra"); };
    if warnings_as_errors { cmd.arg("-Werror"); };
    for warning in ignore_warnings {
        cmd.arg(format!("-Wno-{}", match warning {
            WasmCompileWarning::UnusedFunction => "unused-function",
        }));
    };
    cmd.arg("--target=wasm32-unknown-unknown-wasm")
        .arg("-nostdlib")
        .arg("-nostdinc")
        .arg("-isystemstubs")
        // Prevent optimising from/to functions that don't exist e.g. printf => puts/putchar.
        .arg("-fno-builtin")
        // Needed for import function declarations.
        .arg("-Wl,--allow-undefined")
        .arg("-Wl,--import-memory")
        .arg("-Wl,--export-dynamic")
        .arg("-Wl,--no-entry")
        .arg("-Wl,--strip-all")
    ;
    for (name, code) in macros.iter() {
        cmd.arg(format!("-D{}={}", name, code));
    };
    cmd.arg(input);
    cmd.arg("-o").arg(output);

    if !cmd.status().expect("compile WASM").success() {
        panic!("Failed to compile WASM");
    };
}

pub fn generate_and_compile_runner_wasm(
    output_dir: &PathBuf,
    max_results: usize,
    max_query_terms: usize,
    terms_chunks_raw: &str,
    terms_chunks_len: usize,
    documents_chunks_raw: &str,
    documents_chunks_len: usize,
) -> () {
    let source_path = output_dir.join("runner.c");
    let output_path = output_dir.join("runner.wasm");

    let mut source_file = File::create(&source_path).expect("open runner.c for writing");
    source_file.write_all(RUNNER_C_SYS.as_bytes()).expect("write runner.c");
    source_file.write_all(RUNNER_C_ROARING.as_bytes()).expect("write runner.c");
    let runner_c_index = if cfg!(feature = "nonportable") {
        RUNNER_C_INDEX.replace(
            "roaring_bitmap_portable_deserialize",
            "roaring_bitmap_deserialize",
        )
    } else {
        RUNNER_C_INDEX.to_string()
    };
    source_file.write_all(runner_c_index.as_bytes()).expect("write runner.c");
    source_file.write_all(RUNNER_C_CHUNKS
        .replace("___NORMAL_TERMS_CHUNKS___", terms_chunks_raw)
        .replace("___NORMAL_TERMS_CHUNKS_LEN___", format!("{}", terms_chunks_len).as_str())
        .replace("___DOCUMENTS_CHUNKS___", documents_chunks_raw)
        .replace("___DOCUMENTS_CHUNKS_LEN___", format!("{}", documents_chunks_len).as_str())
        .as_bytes()
    ).expect("write runner.c");

    compile_to_wasm(WasmCompileArgs {
        standard: WasmStandard::C11,
        optimisation_level: WasmOptimisationLevel::Level(3),
        all_warnings: true,
        extra_warnings: true,
        warnings_as_errors: false,
        ignore_warnings: &vec![WasmCompileWarning::UnusedFunction],
        macros: &[
            ("MAX_RESULTS", format!("{}", max_results).as_str()),
            ("MAX_QUERY_TERMS", format!("{}", max_query_terms).as_str()),
        ],
        input: &source_path,
        output: &output_path,
    });
}
