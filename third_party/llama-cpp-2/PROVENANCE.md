# llama-cpp-2, patched

- Source: the crate `llama-cpp-2` 0.1.161 from crates.io (repository https://github.com/utilityai/llama-cpp-rs), copied
  on 17 November 2026. Licence MIT or Apache-2.0.
- Changes:
  - `LlamaModelParams::with_use_extra_bufts` in `src/model/params.rs`, so the runner can keep a mixture-of-experts
    model's experts in the mapped file instead of llama.cpp repacking them into memory (llama-model-loader.cpp: "when
    overriding to a CPU buffer, consider the extra buffer types").
  - `LlamaContextParams::with_cb_eval` in `src/context/params/get_set.rs`: llama.cpp's evaluation callback, which the
    runner uses to see which experts each layer chose and read them from the disk ahead of their use.
- Used through `[patch.crates-io]` in the workspace's `Cargo.toml`. Drop the copy when the upstream crate offers the same.
