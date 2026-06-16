#!/usr/bin/env cargo
---cargo
[dependencies]
prost-build = "0"
---

fn main() -> std::io::Result<()> {
    // https://github.com/google/perfetto/blob/main/protos/perfetto/trace/perfetto_trace.proto
    prost_build::Config::new()
        .format(true)
        .out_dir("src/")
        .compile_protos(&["perfetto_trace.proto"], &["protos"])?;
    Ok(())
}