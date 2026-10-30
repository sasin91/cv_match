# laya-server

Laya's `POST /v1/systemone` on ONNX Runtime, in Rust: a drop-in for `python -m laya.serve`
serving the English checkpoint, so Recruiter can run Laya on a PC without Python. It answers
the same JSON as laya 0.4.0's ONNX backend (`model: "laya-rl-agent-onnx"`), and
`modules/laya/Laya_client.php` talks to it unchanged.

It is a port of laya 0.4.0's `build_head` / `build_sequence` / `collate_items` and the
temperature and decoding rules of `ONNXAgent`. `tests/` checks it against fixtures recorded
from the Python package: the token rows exactly, the answers to the 4 decimals laya reports.

| | PyTorch (laya-serve, CPU) | laya-server |
|---|---|---|
| Recruiter request (2 questions, ~140 tokens) | ~3.5 s | ~90 ms |
| 512-token state | | ~0.9 s |
| Memory | | ~1.5 GB (1.8 GB peak) |

## One-time: export the model (Windows)

The checkpoint's weights are exported to ONNX once, with Python. Nothing here is committed:
the weights, `laya.onnx(.data)` and `tokenizer.json` are in `.gitignore`.

```powershell
cd desktop\laya-server\export
py -3 -m venv .venv
.venv\Scripts\python -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.venv\Scripts\python -m pip install "laya[serve,onnx]==0.4.0" onnxscript

# The English checkpoint (repo root), pinned to the commit the fixtures were made with.
.venv\Scripts\python -c "from huggingface_hub import snapshot_download; snapshot_download('convaiinnovations/laya', revision='7b928d828b7b0e022f929d9bd2e44165aa270148', local_dir='english-7b928d82', allow_patterns=['config.json', 'model.safetensors', 'rl_agent_config.json', 'encoder/*', 'tokenizer/*'])"

# torch.onnx prints emoji, which the default Windows console encoding can't.
$env:PYTHONUTF8 = 1
.venv\Scripts\python export.py english-7b928d82 out\laya.onnx
```

`export.py` ends with `VERIFY PASSED` after checking the graph against PyTorch at batch 1, 2
and 3. It writes `out\laya.onnx` (3 MB) and `out\laya.onnx.data` (1.7 GB); keep them together.

`fixtures.py` re-records `tests/fixtures.json` (12 synthetic Recruiter summaries) if the
checkpoint or laya version ever changes:

```powershell
.venv\Scripts\python fixtures.py english-7b928d82 out\laya.onnx ..\tests\fixtures.json
```

## Run the server

```powershell
cd desktop\laya-server
$env:LAYA_CHECKPOINT = "$PWD\export\english-7b928d82"   # rl_agent_config.json + tokenizer/
$env:LAYA_ONNX = "$PWD\export\out\laya.onnx"
cargo run --release
```

| Variable | | Default |
|---|---|---|
| `LAYA_CHECKPOINT` | folder with `rl_agent_config.json` and `tokenizer/` | required |
| `LAYA_ONNX` | the exported `laya.onnx`, `.data` beside it | required |
| `LAYA_HOST` | bind address | `127.0.0.1` |
| `LAYA_PORT` | bind port | `8080` |
| `LAYA_API_KEY` | if set, requests need `Authorization: Bearer <it>` (401 otherwise) | none |
| `LAYA_THREADS` | cap on ONNX Runtime's threads | all cores |

The first build downloads a prebuilt ONNX Runtime (the `ort` crate does this).

## Point Recruiter at it

In Recruiter's environment:

```
LAYA_URL=http://127.0.0.1:8080
LAYA_API_KEY=<the same key, if you set one>
```

## What it serves

- `GET /health`: `{"status": "ok"}`, plus what is loaded for an authorized caller.
- `POST /v1/systemone` with `{state, questions, model?}`: `state` is a string (or a JSON
  object / turn list, serialized as laya does); questions of type `noul`, `score` and `choice`;
  `model` is omitted or `"english"`.

Errors are FastAPI-shaped, `{"detail": "..."}`: 400 for a malformed body, 401 for a wrong key,
413 past laya-serve's size limits, 422 for a question that can't be answered (the message
names it, worded as laya's). Fields laya-serve acts on but this server does not implement are
refused with 422 rather than ignored: `task`, `lang`, `lang_guess`, `min_confidence`,
`max_len`, `head_max_len`, `hooks`, and per question `labels` and `option_order`.

## Tests

```powershell
cargo test   # unit tests; the model tests skip without the env vars below
$env:LAYA_CHECKPOINT = "..."; $env:LAYA_ONNX = "..."; cargo test --release
```
