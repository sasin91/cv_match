# Licences

JobFinder is under the BSD 2-Clause licence ([LICENSE](LICENSE)). These parts of the repository,
and of the built `jobfinder.exe`, come from others and keep their own licences:

| Part | Where | Licence |
| --- | --- | --- |
| `laya-server/`, the Rust port of Convai Innovations' [laya](https://huggingface.co/convaiinnovations/laya) 0.4.0 | built into the app | Apache-2.0 ([laya-server/LICENSE](laya-server/LICENSE)) |
| Laya's model weights (`laya.onnx`, exported from the English checkpoint) | not in the repository; you export them | the checkpoint's own licence on Hugging Face |
| Trongate MX (`assets/js/trongate-mx.min.js`), from the [Trongate framework](https://github.com/trongate/trongate-framework), copyright (c) 2019 Dave Connelly | built into the app | MIT |
| DejaVu Sans 2.37 (`assets/fonts`) | built into the app, and into the PDFs | Bitstream Vera / DejaVu ([assets/fonts/LICENSE](assets/fonts/LICENSE)) |
| The Rust crates in `Cargo.lock` | built into the app | each crate's own (`cargo metadata` lists them) |
| llama.cpp and the GGUF models | downloaded by the app, never shipped | MIT; each model's own licence |
